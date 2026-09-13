/* SPDX-License-Identifier: GPL-2.0-only */
/* Production module startup; diagnostics never initialize the display or USB. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <sys/utsname.h>
#include <unistd.h>
int main(void)
{
    int state, module, log; pid_t child; cpu_set_t cpu;
    if (access("/sys/module/mediatek_drm", F_OK) == 0) return 0;
    state = open("/run/y2-display-state", O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC, 0644);
    if (state < 0) return errno == EEXIST ? 0 : 1;
    struct utsname system; char path[512];
    if (uname(&system)) {dprintf(state,"uname failed\n");return 1;}
    snprintf(path,sizeof(path),"/run/y2/modules/%s/kernel/mediatek-drm.ko",system.release);
    module = open(path, O_RDONLY|O_CLOEXEC);
    log = open("/dev/kmsg", O_WRONLY|O_CLOEXEC);
    if (module < 0) {dprintf(state,"module open failed errno=%d\n",errno);return 1;}
    child = fork();
    if (child < 0) {dprintf(state,"fork failed errno=%d\n",errno);return 1;}
    if (child) {close(state);close(module);if(log>=0)close(log);return 0;}
    CPU_ZERO(&cpu);CPU_SET(1,&cpu);
    if (sched_setaffinity(0,sizeof(cpu),&cpu)<0) {
        dprintf(state,"affinity failed errno=%d\n",errno);return 1;
    }
    if(log>=0)dprintf(log,"Y2DISPLAY: production module initialization on CPU1\n");
    int rc = syscall(SYS_finit_module,module,"",0), error = rc ? errno : 0;
    dprintf(state,"module result=%d errno=%d\n",rc,error);
    if(log>=0)dprintf(log,"Y2DISPLAY: module result=%d errno=%d\n",rc,error);
    close(state);close(module);if(log>=0)close(log);
    return error ? 1 : 0;
}
