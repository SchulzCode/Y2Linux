/* SPDX-License-Identifier: GPL-2.0-only */
#ifdef ARM
static void finish(unsigned code) {
    register unsigned r0 __asm__("r0")=code,r7 __asm__("r7")=1;
    __asm__ volatile("svc 0" : : "r"(r0),"r"(r7) : "memory");for(;;) {}
}
#define CHECK(x) do {if(!(x)) finish(__LINE__%200+1);} while(0)
#else
#include <assert.h>
#define CHECK assert
#endif
static long call5(long,long,long,long,long,long);
#include "../../initramfs/relay.h"
static const char *incoming;
static unsigned incoming_n,incoming_pos,discovery,block,partial,output_n,kreads,reads,writes;
static unsigned opens,closes,seeked,flushes,termios_count,read_error,write_error,kerror;
static char output[2097152];
static int equal(const char *a,const char *b)
{while(*a && *a==*b) {++a;++b;}return *a==*b;}
static int contains(const char *needle) {
    unsigned size=relay_length(needle);
    for(unsigned i=0;i+size<=output_n;++i) {
        unsigned j=0;while(j<size && output[i+j]==needle[j]) ++j;
        if(j==size) return 1;
    }
    return 0;
}
static long call5(long nr,long a,long b,long c,long d,long e) {
    (void)d;(void)e;
    if(nr==5) {
        ++opens;
        if(equal((char*)a,"/sys/class/tty/ttyGS0/dev")) {CHECK(b==2048);return discovery?10:-2;}
        if(equal((char*)a,"/dev/ttyGS0")) {CHECK(b==(2|256|2048));return 11;}
        CHECK(equal((char*)a,"/dev/kmsg") && b==2048);return 12;
    }
    if(nr==14) {
        CHECK((equal((char*)a,"/dev/ttyGS0") && b==0020600 && c==(240<<8)) ||
              (equal((char*)a,"/dev/kmsg") && b==0020400 && c==267));return 0;
    }
    if(nr==54) {
        CHECK(a==11);
        if(b==0x540b) {CHECK(c==1);++flushes;return 0;}
        CHECK(b==0x5401 || b==0x5402);++termios_count;
        if(b==0x5402) {
            unsigned *t=(void*)c;
            CHECK(!t[0] && !t[1] && t[2]==0x18b2 && !t[3]);
            for(unsigned i=16;i<36;++i) CHECK(!((unsigned char*)c)[i]);
        }
        return 0;
    }
    if(nr==19) {CHECK(a==12 && !b && !c);++seeked;kreads=0;return 0;}
    if(nr==6) {CHECK(a>=10 && a<=12);++closes;return 0;}
    if(nr==3) {
        ++reads;
        if(a==10) {for(unsigned i=0;i<6;++i) ((char*)b)[i]="240:0\n"[i];return 6;}
        if(a==11) {
            if(read_error) return -(long)read_error;
            unsigned n=0;
            while(n<(unsigned)c && incoming_pos<incoming_n) ((char*)b)[n++]=incoming[incoming_pos++];
            return n?(long)n:-11;
        }
        CHECK(a==12);
        if(kerror) {unsigned err=kerror;kerror=0;return -(long)err;}
        if(kreads++<20) {
            const char text[]="6,17,1000,-;kernel fixture\n";
            CHECK(c>=(long)sizeof(text));
            for(unsigned i=0;i<sizeof(text)-1;++i) ((char*)b)[i]=text[i];
            return sizeof(text)-1;
        }
        return -11;
    }
    if(nr==4) {
        CHECK(a==11 && c>0);++writes;
        if(write_error) return -(long)write_error;
        if(block) return -11;
        unsigned n=partial && c>3?3:(unsigned)c;
        CHECK(output_n+n<sizeof(output));
        for(unsigned i=0;i<n;++i) output[output_n++]=((char*)b)[i];return n;
    }
    CHECK(0);return -38;
}
static void command(const char *s) {incoming=s;incoming_n=relay_length(s);incoming_pos=0;}
static void service(void) {
    unsigned r=reads,w=writes;relay_service();CHECK(reads-r<=18 && writes-w<=8);
    CHECK(relay.head-relay.tail<=sizeof(relay.queue));
}
static void run(void) {
    unsigned device;
    CHECK(relay_device("4095:1048575\n",13,&device) && device==0xffffffffU);
    const char *bad[]={"",":1","1:","0:1","4096:1","1:1048576","1:2x","1:2\n3","-1:2","9999999999999:1"};
    for(unsigned i=0;i<sizeof(bad)/sizeof(*bad);++i) CHECK(!relay_device(bad[i],relay_length(bad[i]),&device));
    relay_log("PID1 startup\n",13);
    service();CHECK(relay.tty==-1 && !writes && opens==1);
    discovery=1;service();CHECK(relay.tty==11 && !writes && termios_count==2);
    command("BAD\nLOG1x\nLOG\n");service();CHECK(!relay.active && !writes);
    command("LO");service();CHECK(!relay.active);
    command("G1\n");partial=1;service();CHECK(relay.active && seeked==1 && output_n==24);
    for(unsigned i=0;i<100;++i) service();
    CHECK(contains("Y2LOG1 M2-BASELINE-03\nPID1 startup\n"));
    CHECK(contains("6,17,1000,-;kernel fixture\n") && relay.head==relay.tail);
    relay_log("PID1 live\n",10);partial=0;service();CHECK(contains("PID1 live\n"));
    block=1;
    for(unsigned i=0;i<60000;++i) relay_log("PID1 live\n",10);
    service();CHECK(relay.lost && relay.history_lost);
    block=0;service();service();CHECK(contains("GAP relay queue overflow\n"));
    command("LOG1\n");service();CHECK(seeked==2 && contains("GAP PID1 history full\n"));
    kerror=32;service();CHECK(contains("GAP kernel ring overrun\n"));
    kerror=5;service();CHECK(relay.result==-5 && contains("ERR KMSG read\n"));
    read_error=4;service();CHECK(relay.tty==11);
    read_error=0;write_error=4;relay_text("pending\n");service();CHECK(relay.tty==11);
    relay.result=0; /* isolate expected transport loss from the earlier KMSG fault */
    write_error=5;service();CHECK(relay.tty==-1 && !relay.result && flushes==1);
    write_error=0;service();CHECK(relay.tty==11 && !relay.active);
    read_error=19;service();CHECK(relay.tty==-1 && !relay.result && flushes==2);
    read_error=0;output_n=0;relay_log("PID1 offline heartbeat\n",23);
    service();command("LOG1\n");service();
    CHECK(contains("Y2LOG1 M2-BASELINE-03\n") && !relay.result);
    /* Full-history case still reports its loss; no silent completeness claim. */
    CHECK(contains("GAP PID1 history full\n"));
    relay.history_used=relay.history_lost=0;relay_close();
    relay_log("PID1 offline heartbeat\n",23);output_n=0;
    service();command("LOG1\n");service();CHECK(contains("PID1 offline heartbeat\n"));
    write_error=22;relay_text("pending\n");service();CHECK(relay.result==-22 && relay.tty==-1);
    unsigned count=closes;relay_close();CHECK(closes==count);
}
#ifdef ARM
void _start(void) {run();finish(0);}
#else
int main(void) {run();}
#endif
