// SPDX-License-Identifier: GPL-2.0-only
/* A child process checks the userspace ABI before rescue commits PID1 to SD. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static volatile sig_atomic_t delivered;
__attribute__((target("thumb"),noinline))
static void thumb_handler(int signo) { if(signo==SIGUSR1) delivered=1; }
static void *thread_check(void *arg)
{
    errno=ERANGE;
    return errno==ERANGE ? arg : NULL;
}
int main(void)
{
    setbuf(stdout,NULL);
    alarm(5); /* Parent remains rescue PID1 if this child hangs or faults. */
    puts("Y2ABI begin: Thumb signal return, kuser helpers, glibc TLS/threads and VFP");
    if(!((uintptr_t)thumb_handler & 1))return 9;
    struct sigaction action={0};action.sa_handler=thumb_handler;
    if(sigemptyset(&action.sa_mask)||sigaction(SIGUSR1,&action,NULL)||raise(SIGUSR1)||!delivered) return 1;
    puts("Y2ABI Thumb signal delivery/return OK");
    volatile unsigned *version=(volatile unsigned *)(uintptr_t)0xffff0ffc;
    if(*version<5 || *version>64) {puts("Y2ABI missing supported kuser helper version");return 2;}
    ((void (*)(void))(uintptr_t)0xffff0fa0)(); /* documented memory barrier */
    void *helper_tls=((void *(*)(void))(uintptr_t)0xffff0fe0)();
    void *native_tls;
    __asm__ volatile("mrc p15, 0, %0, c13, c0, 3":"=r"(native_tls));
    if(helper_tls!=native_tls)return 3;
    puts("Y2ABI kuser memory barrier and TLS OK");
    pthread_t thread;int token=37;void *result=NULL;
    if(pthread_create(&thread,NULL,thread_check,&token)||pthread_join(thread,&result)||result!=&token)return 4;
    volatile double a=3.5,b=2.0,c=a*b;
    if(c!=7.0)return 5;
    struct timespec now;if(clock_gettime(CLOCK_MONOTONIC,&now))return 6;
    char *memory=malloc(1024*1024);if(!memory)return 7;
    memset(memory,0xa5,1024*1024);
    for(unsigned i=0;i<1024*1024;i++)if((unsigned char)memory[i]!=0xa5)return 8;
    free(memory);alarm(0);
    puts("Y2ABI PASS: glibc thread/TLS, VFP, clock and allocator");
    return 0;
}
