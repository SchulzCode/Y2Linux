/* SPDX-License-Identifier: GPL-2.0-only
 * Production ARM evdev reader with synthetic syscalls, never hardware input. */
static void finish(unsigned code)
{
    register unsigned r0 __asm__("r0")=code,r7 __asm__("r7")=1;
    __asm__ volatile("svc 0" : : "r"(r0),"r"(r7) : "memory");
    for(;;) {}
}
static void debug(void);
#define CHECK(x) do {if(!(x)) {debug();finish(__LINE__%200+1);}} while(0)
static long call5(long,long,long,long,long,long);
#include "../../initramfs/status.h"
#include "../../initramfs/relay.h"
#include "../../initramfs/evdev.h"
static void debug(void)
{
    register unsigned r0 __asm__("r0")=1,r7 __asm__("r7")=4;
    register char *r1 __asm__("r1")=relay.history;
    register unsigned r2 __asm__("r2")=relay.history_used;
    __asm__ volatile("svc 0" : "+r"(r0) : "r"(r1),"r"(r2),"r"(r7) : "memory");
}
static unsigned scenario,reads,closes,file_kind;
static int equal(const char *a,const char *b)
{while(*a && *a==*b) {++a;++b;}return *a==*b;}
static unsigned copy(char *dst,const char *src)
{unsigned n=0;while(src[n]) {dst[n]=src[n];++n;}return n;}
static int history(const char *needle)
{
    unsigned n=relay_length(needle);
    for(unsigned i=0;i+n<=relay.history_used;++i) {
        unsigned j=0;while(j<n && relay.history[i+j]==needle[j]) ++j;
        if(j==n) return 1;
    }
    return 0;
}
static long call5(long nr,long a,long b,long c,long d,long e)
{
    (void)d;(void)e;
    if(nr==5) {
        CHECK(b==2048);
        if(equal((char*)a,"/dev/y2input")) return scenario==5 ? -13 : 9;
        if(equal((char*)a,"/sys/class/input/event3/dev")) {file_kind=2;return 8;}
        if(equal((char*)a,"/sys/class/input/event3/device/name") ||
           (scenario==2 && equal((char*)a,"/sys/class/input/event2/device/name"))) {
            if(scenario==1) return -2;
            file_kind=1;return 8;
        }
        CHECK(starts((char*)a,"/sys/class/input/event"));return -2;
    }
    if(nr==14) {
        CHECK(equal((char*)a,"/dev/y2input") && b==0020400 && c==((13<<8)|67));
        return scenario==4 ? -17 : 0;
    }
    if(nr==54) {
        CHECK(a==9);
        if((unsigned long)b==0x80404506u) {
            unsigned n=copy((char*)c,scenario==6 ? "Wrong device" : "Y2 navigation buttons");
            ((char*)c)[n]=0;return n+1;
        }
        CHECK((unsigned long)b==0x80404518u);
        for(unsigned i=0;i<64;++i) ((char*)c)[i]=0;
        if(scenario==15) ((char*)c)[28/8]=1<<(28%8);
        return scenario==7 ? 1 : 64;
    }
    if(nr==6) {CHECK(a==8 || a==9);if(a==9) ++closes;return 0;}
    CHECK(nr==3);
    if(a==8) {
        CHECK(c==64);
        return copy((char*)b,file_kind==1 ? "Y2 navigation buttons\n" :
                    scenario==3 ? "13:1048576\n" : "13:67\n");
    }
    CHECK(a==9 && c==256);++reads;
    if(scenario==8) return -4;
    if(scenario==9) return 15;
    if(scenario==10) return -19;
    if(scenario==11) return 0;
    if(scenario==13) return 257;
    if(reads>1 && scenario!=14) return -11;
    struct nav_event *ev=(void*)b;
    for(unsigned i=0;i<16;++i) {
        ev[i].sec=12;ev[i].usec=1000*i;
        ev[i].type=scenario==14 || !(i&1) ? 1 : 0;
        ev[i].code=ev[i].type ? 28 : 0;ev[i].value=!(i&2);
    }
    if(scenario==12) {ev[0].type=0;ev[0].code=3;}
    return scenario==14 ? 256 : 64;
}
void _start(void)
{
    for(scenario=0;scenario<16;++scenario) {
        nav.fd=-1;nav.result=0;nav.sequence=nav.events=0;
        reads=closes=0;relay.history_used=relay.history_lost=0;
        nav_open();nav_service();
        switch(scenario) {
        case 0: case 15:
            CHECK(!nav.result && nav.fd==9 && nav.sequence==4 && nav.events==2);
            CHECK(history("INPUT KEY 1 28 1\n") && history("INPUT KEY 3 28 0\n"));
            CHECK(history(scenario==15 ? "INPUT INITIAL 28 1 0\n" : "INPUT INITIAL 28 0 0\n"));
            break;
        case 1: case 6: CHECK(nav.result==-19);break;
        case 2: case 4: CHECK(nav.result==-17);break;
        case 3: case 7: CHECK(nav.result==-22);break;
        case 5: CHECK(nav.result==-13);break;
        case 8: CHECK(!nav.result && reads==4 && !nav.events);break;
        case 9: case 11: case 13: CHECK(nav.result==-71);break;
        case 10: CHECK(nav.result==-19);break;
        case 12: CHECK(nav.result==-75 && !nav.events && history("INPUT ERROR -75"));break;
        case 14: CHECK(!nav.result && reads==4 && nav.events==64);break;
        }
        if(nav.result) CHECK(nav.fd==-1 && history("INPUT ERROR"));
        nav_close();CHECK(closes<=1 && !relay.history_lost);
    }
    finish(0);
}
