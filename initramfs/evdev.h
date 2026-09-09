/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_EVDEV_H
#define Y2_EVDEV_H
/* ARM EABI input_event uses the 32-bit kernel timeval ABI. No libc, shell,
 * fixed event number, blocking read or raw GPIO access from userspace. */
struct nav_event {
    unsigned sec, usec;
    unsigned short type, code;
    int value;
};
_Static_assert(sizeof(struct nav_event)==16, "ARM evdev ABI");
static struct { long fd, result; unsigned sequence, events; } nav={.fd=-1};
static void nav_log(const char *kind,long a,long b,long c)
{
    char line[Y2_COLS+1];unsigned col;
    row_clear(line);col=row_add(line,0,"INPUT ");col=row_add(line,col,kind);
    col=row_add(line,col," ");col=row_number(line,col,a);
    col=row_add(line,col," ");col=row_number(line,col,b);
    col=row_add(line,col," ");col=row_number(line,col,c);
    line[col++]='\n';relay_log(line,col);
}
static void nav_close(void)
{
    if(nav.fd>=0) call5(6,nav.fd,0,0,0,0);
    nav.fd=-1;
}
static void nav_fail(long result)
{
    nav.result=result;nav_log("ERROR",result,nav.sequence,nav.events);nav_close();
}
static long nav_read_file(const char *path,char *buf,unsigned size)
{
    long fd=call5(5,(long)path,2048,0,0,0),n;
    if(fd<0) return fd;
    n=call5(3,fd,(long)buf,size,0,0);call5(6,fd,0,0,0,0);
    return n;
}
static int nav_name(const char *buf,long n)
{
    static const char expected[]="Y2 navigation buttons";
    if(n!=(long)sizeof(expected) || (buf[n-1]!='\n' && buf[n-1]!=0)) return 0;
    for(unsigned i=0;i<sizeof(expected)-1;++i) if(buf[i]!=expected[i]) return 0;
    return 1;
}
static void nav_open(void)
{
    char namepath[]="/sys/class/input/event0/device/name";
    char devpath[]="/sys/class/input/event0/dev";
    char buf[64];unsigned device,found=0;long n;
    /* Bounded discovery, explicitly unique name; unrelated event devices ignored. */
    for(unsigned i=0;i<8;++i) {
        namepath[sizeof("/sys/class/input/event")-1]='0'+i;
        n=nav_read_file(namepath,buf,sizeof(buf));
        if(!nav_name(buf,n)) continue;
        if(found++) {nav_fail(-17);return;}
        devpath[sizeof("/sys/class/input/event")-1]='0'+i;
    }
    if(!found) {nav_fail(-19);return;}
    n=nav_read_file(devpath,buf,sizeof(buf));
    if(n<=0 || n>=(long)sizeof(buf) || !relay_device(buf,(unsigned)n,&device)) {
        nav_fail(n<0?n:-22);return;
    }
    n=call5(14,(long)"/dev/y2input",0020400,device,0,0);
    if(n<0) {nav_fail(n);return;} /* Fresh initramfs; EEXIST is unexpected. */
    nav.fd=call5(5,(long)"/dev/y2input",2048,0,0,0);
    if(nav.fd<0) {nav_fail(nav.fd);return;}
    n=call5(54,nav.fd,0x80404506,(long)buf,0,0); /* EVIOCGNAME(64) */
    if(!nav_name(buf,n)) {nav_fail(n<0?n:-19);return;}
    nav_log("OPEN",device,0,0);
    /* Presses held before open have no queued edge: retain the initial key bitmap. */
    n=call5(54,nav.fd,0x80404518,(long)buf,0,0); /* EVIOCGKEY(64) */
    if(n<0 || n>64) {nav_fail(n<0?n:-75);return;}
    for(unsigned i=0;i<5;++i) {
        static const unsigned codes[]={105,158,106,164,28};
        unsigned code=codes[i];
        if(n<=(long)(code/8)) {nav_fail(-22);return;}
        nav_log("INITIAL",code,((unsigned char)buf[code/8]>>(code%8))&1,0);
    }
}
static void nav_service(void)
{
    struct nav_event events[16];
    if(nav.fd<0) return;
    /* <=64 events per heartbeat. Polling happens in the upstream input driver. */
    for(unsigned batch=0;batch<4;++batch) {
        long n=call5(3,nav.fd,(long)events,sizeof(events),0,0);
        if(n==-11) return;
        if(n==-4) continue;
        if(n<=0 || n>(long)sizeof(events) || n%sizeof(events[0])) {
            nav_fail(n<0?n:-71);return;
        }
        for(unsigned i=0;i<(unsigned)n/sizeof(events[0]);++i) {
            const struct nav_event *event=&events[i];
            ++nav.sequence;
            if(event->type==0 && event->code==3) { /* SYN_DROPPED */
                nav_fail(-75);return; /* Do not report later edges as lossless. */
            }
            if(event->type==1) {
                ++nav.events;
                nav_log("KEY",nav.sequence,event->code,event->value);
            } else if(event->type==0 && event->code==0)
                nav_log("SYN",nav.sequence,event->sec,event->usec);
        }
    }
}
#endif
