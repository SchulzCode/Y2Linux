/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_RELAY_H
#define Y2_RELAY_H
/* Freestanding ARM EABI. No shell; the only accepted command is LOG1\n.
 * Every service call is bounded, including no-reader and malformed input. */
static struct {
    long tty, kmsg, result;
    unsigned active, command, bad, head, tail, lost, history_used, history_lost;
    char queue[32768], history[8192], record[8192];
} relay={.tty=-1,.kmsg=-1};
static unsigned relay_length(const char *s)
{ unsigned n=0;while(s[n]) ++n;return n; }
static int relay_put(const char *s,unsigned n)
{
    if(n>sizeof(relay.queue)-(relay.head-relay.tail)) {++relay.lost;return 0;}
    for(unsigned i=0;i<n;++i) relay.queue[relay.head++ & (sizeof(relay.queue)-1)]=s[i];
    return 1;
}
static int relay_text(const char *s) {return relay_put(s,relay_length(s));}
static void relay_log(const char *s,unsigned n)
{
    if(n<=sizeof(relay.history)-relay.history_used) {
        for(unsigned i=0;i<n;++i) relay.history[relay.history_used++]=s[i];
    } else ++relay.history_lost;
    if(relay.active) relay_put(s,n);
}
/* Linux new_encode_dev, rejecting overflows and malformed sysfs content. */
static int relay_device(const char *s,unsigned n,unsigned *device)
{
    unsigned a=0,b=0,i=0,start;
    while(i<n && s[i]>='0' && s[i]<='9') {
        a=a*10+s[i++]-'0';if(a>4095) return 0;
    }
    if(!i || i==n || s[i++]!=':') return 0;
    start=i;
    while(i<n && s[i]>='0' && s[i]<='9') {
        b=b*10+s[i++]-'0';if(b>1048575) return 0;
    }
    if(i==start || !a || (i<n && s[i++]!='\n') || i!=n) return 0;
    *device=(b&255)|(a<<8)|((b&~255U)<<12);return 1;
}
static void relay_close(void)
{
    if(relay.tty>=0) {
        /* Discard queued tty output before close; never wait for a reader. */
        call5(54,relay.tty,0x540b,1,0,0);
        call5(6,relay.tty,0,0,0,0);
    }
    if(relay.kmsg>=0) call5(6,relay.kmsg,0,0,0,0);
    relay.tty=relay.kmsg=-1;relay.active=0;
    relay.head=relay.tail=relay.command=relay.bad=0;
}
static void relay_link_error(long rc)
{
    relay_close();
    /* TTY hangup may precede the 250ms CHRDET worker. Retain the event without
     * latching an expected disconnect as a PID1 error. Other errors still fail. */
    if(rc==-5 || rc==-19 || rc==-32)
        relay_log("LINK tty disconnected\n",22);
    else relay.result=rc;
}
static void relay_open(void)
{
    char dev[24];unsigned device;
    long fd,n,rc;
    struct {unsigned iflag,oflag,cflag,lflag;unsigned char line,cc[19];} t;
    fd=call5(5,(long)"/sys/class/tty/ttyGS0/dev",2048,0,0,0);
    if(fd<0) return; /* Enumeration/UDC registration can happen later. */
    n=call5(3,fd,(long)dev,sizeof(dev),0,0);call5(6,fd,0,0,0,0);
    if(n<0 || n>=(long)sizeof(dev) || !relay_device(dev,(unsigned)n,&device)) {
        relay.result=n<0?n:-22;return;
    }
    rc=call5(14,(long)"/dev/ttyGS0",0020600,device,0,0);
    if(rc<0 && rc!=-17) {relay.result=rc;return;}
    fd=call5(5,(long)"/dev/ttyGS0",2|256|2048,0,0,0);
    if(fd<0) {relay_link_error(fd);return;}
    relay.tty=fd;
    rc=call5(54,fd,0x5401,(long)&t,0,0);
    if(rc>=0) {
        t.iflag=t.oflag=t.lflag=0;t.cflag=0x800|0x80|0x30|0x1002;
        t.line=0;for(unsigned i=0;i<19;++i) t.cc[i]=0;
        rc=call5(54,fd,0x5402,(long)&t,0,0);
    }
    if(rc<0) {relay_link_error(rc);return;}
    rc=call5(14,(long)"/dev/kmsg",0020400,(1<<8)|11,0,0);
    if(rc<0 && rc!=-17) relay.result=rc;
    else {
        relay.kmsg=call5(5,(long)"/dev/kmsg",2048,0,0,0);
        if(relay.kmsg<0) relay.result=relay.kmsg;
    }
}
static void relay_request(void)
{
    static const char header[]="Y2LOG1 M2-INPUT-01\n";
    relay.head=relay.tail=0;relay.lost=0;relay.active=1;
    relay_put(header,sizeof(header)-1);
    relay_put(relay.history,relay.history_used);
    if(relay.history_lost) relay_text("GAP PID1 history full\n");
    if(relay.kmsg>=0) {
        long rc=call5(19,relay.kmsg,0,0,0,0); /* oldest available kernel record */
        if(rc<0) {relay.result=rc;relay_text("ERR KMSG rewind\n");}
    } else relay_text("ERR KMSG unavailable\n");
}
static void relay_service(void)
{
    char command[32];long n;
    if(relay.tty<0) relay_open();
    if(relay.tty<0) return;
    n=call5(3,relay.tty,(long)command,sizeof(command),0,0);
    if(n<0 && n!=-11 && n!=-4) {relay_link_error(n);return;}
    for(long i=0;i<n;++i) {
        char c=command[i];
        if(c=='\n') {
            if(!relay.bad && relay.command==4) relay_request();
            relay.command=relay.bad=0;
        } else if(!relay.bad && relay.command<4 && c=="LOG1"[relay.command]) ++relay.command;
        else relay.bad=1;
    }
    if(!relay.active) return;
    if(relay.lost && relay_text("GAP relay queue overflow\n")) relay.lost=0;
    /* Leave room for a complete maximum kmsg record; never split/drop it
     * merely because the host stopped draining the queue. */
    for(unsigned i=0;i<16 && relay.kmsg>=0 &&
        sizeof(relay.queue)-(relay.head-relay.tail)>=sizeof(relay.record);++i) {
        n=call5(3,relay.kmsg,(long)relay.record,sizeof(relay.record),0,0);
        if(n==-11 || !n) break;
        if(n==-4) continue;
        if(n==-32) {relay_text("GAP kernel ring overrun\n");continue;}
        if(n<0) {relay.result=n;relay_text("ERR KMSG read\n");break;}
        relay_put(relay.record,(unsigned)n);
    }
    for(unsigned i=0;i<8 && relay.head!=relay.tail;++i) {
        unsigned offset=relay.tail & (sizeof(relay.queue)-1);
        unsigned count=relay.head-relay.tail;
        if(count>sizeof(relay.queue)-offset) count=sizeof(relay.queue)-offset;
        n=call5(4,relay.tty,(long)(relay.queue+offset),count,0,0);
        if(n==-11 || !n) break;
        if(n==-4) continue;
        if(n<0) {relay_link_error(n);return;}
        relay.tail+=(unsigned)n;
    }
}
#endif
