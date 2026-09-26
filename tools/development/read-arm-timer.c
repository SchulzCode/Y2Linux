// SPDX-License-Identifier: GPL-2.0-only
/* Read-only PL0 counter observation. A trapped instruction is reported as
 * unavailable; this never writes CNTKCTL, a timer, an interrupt or MMIO.
 * This cannot establish privileged CNTP interrupt routing or wake behavior. */
#define _GNU_SOURCE
#include <inttypes.h>
#include <sched.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>

static sigjmp_buf trapped;
static volatile sig_atomic_t probing;
static void unavailable(int signal)
{
	if (probing) siglongjmp(trapped, signal);
	_exit(128 + signal);
}
static int counter(unsigned kind, uint64_t *value)
{
	uint32_t low, high = 0;
	if (sigsetjmp(trapped, 1)) { probing = 0; return -1; }
	probing = 1;
	if (kind == 0)
		asm volatile("mrc p15, 0, %0, c14, c0, 0" : "=r" (low));
	else if (kind == 1)
		asm volatile("isb; mrrc p15, 0, %0, %1, c14" : "=r" (low), "=r" (high));
	else
		asm volatile("isb; mrrc p15, 1, %0, %1, c14" : "=r" (low), "=r" (high));
	probing = 0;
	*value = ((uint64_t)high << 32) | low;
	return 0;
}
static uint64_t nanoseconds(void)
{
	struct timespec time;
	if (clock_gettime(CLOCK_MONOTONIC_RAW, &time)) return 0;
	return (uint64_t)time.tv_sec * 1000000000 + time.tv_nsec;
}
int main(void)
{
	struct sigaction action = { .sa_handler = unavailable };
	cpu_set_t original, one;
	unsigned cpu, kind;
	const char *names[] = { "CNTFRQ", "CNTPCT", "CNTVCT" };
	sigemptyset(&action.sa_mask);
	if (sigaction(SIGILL, &action, NULL) || sigaction(SIGSEGV, &action, NULL) ||
	    sched_getaffinity(0, sizeof(original), &original)) return 1;
	for (cpu = 0; cpu < 4; cpu++) {
		if (!CPU_ISSET(cpu, &original)) continue;
		CPU_ZERO(&one); CPU_SET(cpu, &one);
		if (sched_setaffinity(0, sizeof(one), &one)) return 1;
		for (kind = 0; kind < 3; kind++) {
			uint64_t first = 0, last = 0, before = 0, after = 0;
			int result = counter(kind, &first);
			if (!result && kind) {
				struct timespec delay = { .tv_nsec = 200000000 };
				before = nanoseconds();
				while (nanosleep(&delay, &delay)) {}
				result = counter(kind, &last);
				after = nanoseconds();
			}
			printf("{\"cpu\":%u,\"register\":\"%s\",\"available\":%s,"
			       "\"first\":%" PRIu64 ",\"last\":%" PRIu64 ",\"elapsed_ns\":%" PRIu64 "}\n",
			       cpu, names[kind], result ? "false" : "true", first, last, after - before);
		}
	}
	return sched_setaffinity(0, sizeof(original), &original) ? 1 : 0;
}
