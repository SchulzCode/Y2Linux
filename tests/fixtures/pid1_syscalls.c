/* SPDX-License-Identifier: GPL-2.0-only
 * Host-only QEMU ARM fixture. No actual kernel/device/MMIO is exercised.
 */
#include "text.h"
#include "usb_clock.h"
static unsigned writes,sleeps,opened,offset,closed;
static unsigned power_reads;
static const char *path;
static const struct y2_text_frame *last;
static int eq(const char *a,const char *b)
{ while(*a && *a==*b) {++a;++b;}return *a==*b; }
static int prefix(const char *a,const char *b)
{ while(*b) if(*a++!=*b++) return 0;return 1; }
static void finish(unsigned code)
{
    register unsigned r0 __asm__("r0")=code;
    register unsigned r7 __asm__("r7")=1;
    __asm__ volatile("svc 0" : : "r"(r0),"r"(r7) : "memory");
    for(;;) {}
}
#define CHECK(x) do {if(!(x)) finish(__LINE__%200+1);} while(0)
long y2_test_call(long number,long a,long b,long c,long d,long e)
{
    (void)d;(void)e;
    if(number==20) return 1;
    if(number==122) {
        char *uts=(char*)a;const char release[]="6.18.0-y2-m2-usbclk1";
        if(TEST_CASE==7) return -14;
        for(unsigned i=0;i<390;++i) uts[i]=0;
        for(unsigned i=0;i<sizeof(release);++i) uts[130+i]=release[i];
        return 0;
    }
    if(number==21) {
        CHECK((eq((char*)b,"/proc") && d==14) || (eq((char*)b,"/sys") && d==15));
        return (TEST_CASE==1 && eq((char*)b,"/proc")) ||
               (TEST_CASE==8 && eq((char*)b,"/sys")) ? -1 : 0;
    }
    if(number==5) {
        if(eq((char*)a,"/dev/y2diag")) { CHECK(b==2);return TEST_CASE==6 ? -6 : 3; }
        CHECK(b==2048 && !opened);
        if(TEST_CASE==1 && prefix((char*)a,"/proc/")) return -2;
        if(TEST_CASE==8 && prefix((char*)a,"/sys/")) return -2;
        if(TEST_CASE==2 && eq((char*)a,"/proc/meminfo")) return -13;
        path=(char*)a;offset=0;opened=1;return 4;
    }
    if(number==4) {
        CHECK(a==3); /* Trap any production console write. */
        CHECK(c==sizeof(struct y2_text_frame));last=(const void*)b;
        CHECK(y2_text_valid(last));++writes;CHECK(writes<1000);return c;
    }
    if(number==3) {
        const char *data="";unsigned size=0,n=0;
        if(a==3) {
            CHECK(c==sizeof(struct y2_platform_snapshot) && sleeps==1);
            CHECK(++power_reads==1);
            CHECK(prefix(last->rows[1],"STAGE: READ PWRAP"));
            if(TEST_CASE==9) return -4;
            if(TEST_CASE==10) return c-1;
            struct y2_platform_snapshot *snapshot=(void*)b;
            struct y2_pwrap_snapshot *s=&snapshot->power;
            snapshot->clock=(struct y2_usb_clock_snapshot){.valid=15,.peri=0x03ffffff,
                .mux=0x00800000,.pll=0xf4000001,.pll_power=1};
            if(TEST_CASE==13) { snapshot->clock.result=-16; snapshot->clock.valid=1; }
            *s=(struct y2_pwrap_snapshot){.magic=Y2_PWRAP_MAGIC,
                .valid=3,.wrap=1,.channel=1,.init=1,.arb=0x1ff,
                .before=0x00300000,.after=0x00300000,.cid=0x2023,.vusb=0xc001};
            if(TEST_CASE==11) {s->result=-110;s->valid=1;}
            if(TEST_CASE==12) s->magic=0;
            return c;
        }
        CHECK(a==4 && opened && c>0 && c<=4096);
        if(TEST_CASE==5) return -4;
        if(TEST_CASE==4 && eq(path,"/proc/meminfo")) {
            for(n=0;n<(unsigned)c;++n) ((char*)b)[n]='X';return c;
        }
        if(eq(path,"/proc/cpuinfo")) data="processor : 0\nCPU part\t: 0xc07\n";
        else if(eq(path,"/sys/devices/system/cpu/online")) data="0\n";
        else if(eq(path,"/proc/meminfo")) data="MemTotal: 21000 kB\nMemFree: 17000 kB\n";
        else if(eq(path,"/proc/uptime")) data="12.34 5.67\n";
        else if(eq(path,"/proc/interrupts")) data="CPU0\n 1: 1234 GIC 112 Level timer@10008000\n";
        else if(eq(path,"/sys/firmware/devicetree/base/compatible")) {
            static const char compat[]="innioasis,y2\0mediatek,mt6582";
            data=compat;size=sizeof(compat);
        } else if(eq(path,"/sys/firmware/devicetree/base/memory@80000000/reg")) {
            static const unsigned char ram[]={0x80,0,0,0,1,0x80,0,0,0x84,0,0,0,0,8,0,0};
            data=(const char*)ram;size=sizeof(ram);
        } else CHECK(0);
        if(!size) while(data[size]) ++size;
        while(offset<size && n<(unsigned)c) ((char*)b)[n++]=data[offset++];
        return n;
    }
    if(number==6) {
        if(a==4) { CHECK(opened);opened=0;++closed; }
        else CHECK(a==3);
        return 0;
    }
    if(number==162) {
        const long *ts=(const long*)a;CHECK(ts[0]==1 && ts[1]==0);
        ++sleeps;CHECK(sleeps<=50);
        return TEST_CASE==3 ? -22 : 0;
    }
    if(number==29) {
        CHECK(!opened);
        if(TEST_CASE==6) { CHECK(writes==0 && sleeps==0);finish(0); }
        CHECK(last && writes>=4);
        if(TEST_CASE==3) {
            CHECK(sleeps==1 && prefix(last->rows[1],"STAGE: STOP: SLEEP ERROR"));
            CHECK(prefix(last->rows[13],"SLEEP RC: -22"));finish(0);
        }
        CHECK(sleeps==50 && prefix(last->rows[0],"PID1:1 BEAT:50"));
        CHECK(power_reads==1);
        CHECK(prefix(last->rows[1],"STAGE: STOP: RESTORE ANDROID"));
        CHECK(closed>0);
        if(TEST_CASE==0) {
            CHECK(prefix(last->rows[2],"LINUX: 6.18.0-y2-m2-usbclk1"));
            CHECK(prefix(last->rows[3],"CPU PART: 0xc07"));
            CHECK(prefix(last->rows[4],"PWRAP RC:0"));
            CHECK(prefix(last->rows[4]+20,"VALID:3"));
            CHECK(prefix(last->rows[16],"CID:00002023 VUSB:0000C001"));
            CHECK(prefix(last->rows[17],"PERI:03FFFFFF MUX:00800000"));
            CHECK(prefix(last->rows[18],"PLL:F4000001 PWR:00000001"));
            CHECK(prefix(last->rows[19],"CLOCK RC:0"));
            CHECK(prefix(last->rows[19]+18,"VALID:15 READ ONLY"));
            CHECK(prefix(last->rows[6],"DT RAM: 24M + 512K (D08 STATIC)"));
            CHECK(prefix(last->rows[9],"TIMER IRQ CPU0: 1234"));
        }
        if(TEST_CASE==1) CHECK(prefix(last->rows[10],"PROC MOUNT RC: -1"));
        if(TEST_CASE==2) CHECK(prefix(last->rows[7],"MEMTOTAL: -13"));
        if(TEST_CASE==4) CHECK(prefix(last->rows[7],"MEMTOTAL: -75"));
        if(TEST_CASE==5) CHECK(prefix(last->rows[7],"MEMTOTAL: -11"));
        if(TEST_CASE==7) CHECK(prefix(last->rows[2],"LINUX ERR: -14"));
        if(TEST_CASE==8) CHECK(prefix(last->rows[11],"SYSFS MOUNT RC: -1"));
        if(TEST_CASE==9) CHECK(prefix(last->rows[4],"PWRAP READ ERR: -4"));
        if(TEST_CASE==10 || TEST_CASE==12) CHECK(prefix(last->rows[4],"PWRAP READ ERR: -5"));
        if(TEST_CASE==13) CHECK(prefix(last->rows[19],"CLOCK RC:-16"));
        if(TEST_CASE==11) {
            CHECK(prefix(last->rows[4],"PWRAP RC:-110"));
            CHECK(prefix(last->rows[4]+20,"VALID:1"));
        }
        finish(0);
    }
    CHECK(0);return -38;
}
