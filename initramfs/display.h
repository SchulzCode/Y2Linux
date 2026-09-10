/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_DISPLAY_H
#define Y2_DISPLAY_H
/* The sole packaged module is loaded once, in a separate process. PID1 never
 * calls finit_module or waits synchronously for display probe completion. */
static struct {
    long pid;
    unsigned attempted, started, pending;
} display;

static unsigned display_message(char *buf, const char *text, long value)
{
    unsigned n=0, used=0, v;char digits[12];
    while(*text && n<64) buf[n++]=*text++;
    if(value<0) {buf[n++]='-';v=0U-(unsigned)value;} else v=(unsigned)value;
    do {digits[used++]=(char)('0'+v%10);v/=10;} while(v);
    while(used) buf[n++]=digits[--used];
    buf[n++]='\n';return n;
}
static void display_log(const char *text,long value)
{
    char buf[96];unsigned n=display_message(buf,text,value);
    relay_log(buf,n);
}
static long display_child(void)
{
    unsigned cpu=2;long fd,log,rc;char buf[96];unsigned n;
    /* The logging process and USB IRQs retain CPU0. No CLONE_VM/CLONE_FILES. */
    rc=call5(241,0,sizeof(cpu),(long)&cpu,0,0);
    if(rc<0) return rc;
    if(relay.tty>=0) call5(6,relay.tty,0,0,0,0);
    if(relay.kmsg>=0) call5(6,relay.kmsg,0,0,0,0);
    fd=call5(5,(long)"/display.ko",0,0,0,0);
    if(fd<0) return fd;
    log=call5(5,(long)"/dev/kmsg",1,0,0,0);
    if(log>=0) {
        static const char msg[]="Y2DISPLAY: finit_module begin on CPU1\n";
        call5(4,log,(long)msg,sizeof(msg)-1,0,0);
    }
    rc=call5(379,fd,(long)"",0,0,0); /* ARM EABI finit_module, flags=0 */
    if(log>=0) {
        n=display_message(buf,"Y2DISPLAY: finit_module result=",rc);
        call5(4,log,(long)buf,n,0,0);call5(6,log,0,0,0,0);
    }
    call5(6,fd,0,0,0,0);
    return rc;
}
static void display_service(unsigned beat)
{
    long rc;unsigned cpu=1;int status=0;
    /* Finish the one-time clock/regulator snapshots before probing DRM. */
    if(beat>12 && !display.attempted && relay.active && relay.tail && !relay.result) {
        display.attempted=1;
        if(beat>=240) {display_log("DISPLAY skipped late session, beat=",beat);return;}
        rc=call5(241,0,sizeof(cpu),(long)&cpu,0,0);
        if(rc<0) {display_log("DISPLAY parent affinity failed=",rc);return;}
        display_log("DISPLAY start after LOG1, beat=",beat);
        rc=call5(120,17,0,0,0,0); /* clone(SIGCHLD), separate address space/files */
        if(!rc) {
            rc=display_child();
            call5(1,rc<0 ? 1 : 0,0,0,0,0);
            for(;;) call5(29,0,0,0,0,0);
        }
        if(rc<0) {display_log("DISPLAY fork failed=",rc);return;}
        display.pid=rc;display.started=beat;
        display_log("DISPLAY child pid=",rc);
    }
    if(display.pid<=0) return;
    rc=call5(114,display.pid,(long)&status,1,0,0); /* wait4(WNOHANG) */
    if(rc>0) {display_log("DISPLAY child wait status=",status);display.pid=0;}
    else if(rc<0 && rc!=-4) {display_log("DISPLAY wait error=",rc);display.pid=0;}
    else if(beat-display.started>=display.pending+15) {
        display.pending=beat-display.started;
        display_log("DISPLAY child pending seconds=",display.pending);
    }
}
#endif
