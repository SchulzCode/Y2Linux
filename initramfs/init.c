/* SPDX-License-Identifier: GPL-2.0-only
 * Deliberately tiny diagnostic PID1. No shell, block I/O, network or reboot.
 * ARM EABI numbers: upstream v6.18 arch/arm/tools/syscall.tbl.
 */
typedef unsigned int u32;
static long call5(long number, long a, long b, long c, long d, long e)
{
    register long r0 __asm__("r0") = a;
    register long r1 __asm__("r1") = b;
    register long r2 __asm__("r2") = c;
    register long r3 __asm__("r3") = d;
    register long r4 __asm__("r4") = e;
    register long r7 __asm__("r7") = number;
    __asm__ volatile("svc 0" : "+r"(r0) : "r"(r1), "r"(r2), "r"(r3), "r"(r4), "r"(r7) : "memory", "cc");
    return r0;
}
static void write_all(const char *p, unsigned n)
{
    while (n) {
        long r = call5(4, 1, (long)p, n, 0, 0);
        if (r == -4) continue; /* EINTR */
        if (r <= 0) return;
        p += r; n -= (unsigned)r;
    }
}
static void say(const char *p)
{
    unsigned n = 0;
    while (p[n]) ++n;
    write_all(p, n);
}
static void result(const char *what, long code)
{
    char hex[11] = "0x00000000\n";
    u32 v = (u32)code;
    for (unsigned i = 0; i < 8; ++i)
        hex[9-i] = "0123456789abcdef"[(v >> (i*4)) & 15];
    say(what); write_all(hex, 11);
}
static int equal(const char *a, const char *b)
{
    while (*a && *a == *b) { ++a; ++b; }
    return *a == *b;
}
static void dump(const char *path)
{
    char buf[512];
    unsigned remaining = 16384;
    say("Y2DIAG FILE "); say(path); say("\n");
    long fd = call5(5, (long)path, 0, 0, 0, 0);
    if (fd < 0) { result("Y2DIAG open error ", fd); return; }
    while (remaining) {
        unsigned count = remaining < sizeof(buf) ? remaining : sizeof(buf);
        long n = call5(3, fd, (long)buf, count, 0, 0);
        if (n == -4) continue;
        if (n <= 0) { if (n < 0) result("Y2DIAG read error ", n); break; }
        write_all(buf, (unsigned)n); remaining -= (unsigned)n;
    }
    if (!remaining) say("\nY2DIAG truncated at 16384 bytes\n");
    call5(6, fd, 0, 0, 0, 0);
    say("\nY2DIAG END FILE\n");
}
__attribute__((noreturn)) void diag_start(u32 *stack)
{
    unsigned argc = stack[0];
    char **argv = (char **)(stack + 1);
    if (argc == 2 && equal(argv[1], "--selftest")) {
        say("Y2DIAG SELFTEST ARM EABI OK\n");
        call5(1, 0, 0, 0, 0, 0);
        for (;;) { }
    }
    if (call5(20, 0, 0, 0, 0, 0) != 1) {
        say("Y2DIAG requires PID1 or --selftest\n");
        call5(1, 2, 0, 0, 0, 0);
        for (;;) { }
    }
    long fd = call5(5, (long)"/dev/console", 2, 0, 0, 0);
    if (fd >= 0) {
        for (int i = 0; i < 3; ++i) call5(63, fd, i, 0, 0, 0);
        if (fd > 2) call5(6, fd, 0, 0, 0, 0);
    }
    say("Y2DIAG PID1 ENTER D08 CPU0\n");
    result("Y2DIAG console fd ", fd);
    result("Y2DIAG proc mount ", call5(21, (long)"proc", (long)"/proc", (long)"proc", 15, 0));
    result("Y2DIAG sysfs mount ", call5(21, (long)"sysfs", (long)"/sys", (long)"sysfs", 15, 0));
    dump("/proc/version"); dump("/proc/cmdline"); dump("/proc/meminfo");
    dump("/proc/iomem"); dump("/proc/interrupts"); dump("/proc/cpuinfo");
    dump("/sys/devices/system/cpu/online");
    say("Y2DIAG READY observation only\n");
    for (;;) {
        struct { long sec, nsec; } delay = {5, 0}, rest;
        while (call5(162, (long)&delay, (long)&rest, 0, 0, 0) == -4) delay = rest;
        say("Y2DIAG HEARTBEAT\n");
    }
}
