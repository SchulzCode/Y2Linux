/* SPDX-License-Identifier: GPL-2.0-only
 * Host-only QEMU ARM fixture. No actual kernel/device/MMIO is exercised.
 */
#include "text.h"
#include "usb_state.h"
#include "../../kernel/usb/live.h"
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
        char *uts=(char*)a;const char release[]="6.18.0-y2-m2-usbacm4";
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
        if(eq((char*)a,"/sys/class/tty/ttyGS0/dev")) {CHECK(TEST_CASE==27 || TEST_CASE==28);return -2;}
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
            if(c==sizeof(struct y2_usb_live)) {
                volatile unsigned *words=(void*)b;
                for(unsigned i=0;i<sizeof(struct y2_usb_live)/4;++i) words[i]=0;
                words[0]=Y2_USB_LIVE_MAGIC;
                if(TEST_CASE>=26) {
                    struct y2_usb_live *live=(void*)b;
                    live->stage=TEST_CASE==26 ? Y2_USB_ATTACH : TEST_CASE==27 ? Y2_USB_READY :
                        TEST_CASE==28 ? Y2_USB_CONFIGURED : Y2_USB_PREFLIGHT;
                    live->result=TEST_CASE==29 ? -19 : 0;
                    live->polls=1;live->chrdet=TEST_CASE==29 ? 0x10000 : TEST_CASE==26 ? 1 : 0x7b;
                    live->devctl=TEST_CASE==26 || TEST_CASE==29 ? 0x100 : 0x98;
                    live->irqs=TEST_CASE==28 ? 42 : 0;
                    if(TEST_CASE>=30 && TEST_CASE<=32) {
                        live->stage=Y2_USB_ATTACH;live->result=TEST_CASE==32 ? -110 : -16;
                        live->devctl=0x100;live->chrdet=0x10000;
                        live->power_failure=(struct y2_pwrap_snapshot){
                            .magic=Y2_PWRAP_MAGIC,.result=live->result,
                            .valid=TEST_CASE==32 ? 3 : 0,.wrap=1,.arb=0x1ff,.channel=1,.init=1,
                            .before=TEST_CASE==30 ? 0x00200001 : 0x00360001,
                            .after=TEST_CASE==32 ? 0x00340023 : TEST_CASE==30 ? 0x00200001 : 0x00360001};
                    }
                    if(TEST_CASE==33) {live->stage=Y2_USB_DETACHED;live->result=0;live->polls=50;live->chrdet=1;}
                }
                return c;
            }
            CHECK(c==sizeof(struct y2_platform_snapshot) && sleeps==1);
            CHECK(++power_reads==1);
            CHECK(prefix(last->rows[1],"STAGE: CHECK PHY / RELEASE SUSPEND"));
            if(TEST_CASE==9) return -4;
            if(TEST_CASE==10) return c-1;
            struct y2_platform_snapshot *snapshot=(void*)b;
            struct y2_pwrap_snapshot *s=&snapshot->power;
            snapshot->clock=(struct y2_usb_clock_snapshot){.valid=15,.peri=0,
                .mux=0x01010100,.pll=0xfd000001,.pll_power=0x80000001};
            if(TEST_CASE==13) { snapshot->clock.result=-16; snapshot->clock.valid=1; }
            snapshot->usb.result=0;snapshot->usb.valid=0x1fffff;
            for(unsigned i=0;i<21;++i) snapshot->usb.values[i]=0;
            snapshot->usb.values[0]=0x20;snapshot->usb.values[1]=0x80;
            snapshot->usb.values[2]=0x1900;snapshot->usb.values[3]=0x1234;
            snapshot->usb.values[4]=0x5678;snapshot->usb.values[5]=0x9a;
            for(unsigned i=6;i<13;++i) snapshot->usb.values[i]=i;
            if(TEST_CASE==14) {snapshot->usb.result=-16;snapshot->usb.valid=0;}
            if(TEST_CASE==15) {snapshot->clock.peri=1<<10;snapshot->usb.result=-19;snapshot->usb.valid=0;}
            if(TEST_CASE==16) {snapshot->usb.result=-5;snapshot->usb.valid=1;}
            snapshot->wake.result=0;snapshot->wake.written=1;
            snapshot->wake.before_valid=0x7f;snapshot->wake.after_valid=0xfff;
            for(unsigned i=0;i<7;++i) snapshot->wake.controls[i]=0;
            for(unsigned i=0;i<12;++i) snapshot->wake.after[i]=0;
            snapshot->usb.values[8]=4;
            snapshot->wake.controls[3]=0x10;
            snapshot->wake.after[3]=2;snapshot->wake.after[4]=0x12;
            snapshot->wake.after[10]=0x20;snapshot->wake.after[11]=0x80;
            if(TEST_CASE==17) {snapshot->wake.result=-19;snapshot->wake.written=0;snapshot->wake.after_valid=0;}
            if(TEST_CASE==18) snapshot->wake.result=-5;
            if(TEST_CASE==19) {snapshot->wake.result=-16;snapshot->wake.written=0;snapshot->wake.after_valid=0;}
            if(TEST_CASE==22 || TEST_CASE==25) {
                /* The connected photo establishes 6a=BE and early refusal.
                 * Other values here are synthetic sentinels, not device evidence. */
                snapshot->usb.values[8]=0xbe;
                snapshot->usb.values[13]=0xabcd;
                snapshot->usb.values[20]=0x1234;
                snapshot->wake.result=TEST_CASE==22 ? -19 : -12;
                snapshot->wake.written=0;snapshot->wake.before_valid=0;
                snapshot->wake.after_valid=0;
            }
            if(TEST_CASE==23) {
                snapshot->wake.result=-12;snapshot->wake.written=0;
                snapshot->wake.before_valid=7;snapshot->wake.after_valid=0;
            }
            if(TEST_CASE==24) {
                snapshot->wake.result=-12;snapshot->wake.after_valid=3;
            }
            *s=(struct y2_pwrap_snapshot){.magic=Y2_PWRAP_MAGIC,
                .valid=7,.wrap=1,.channel=1,.init=1,.arb=0x1ff,
                .before=0x00300000,.after=0x00300000,.cid=0x2023,.vusb=0xc001,.chrdet=0xa520};
            if(TEST_CASE==20) s->chrdet=0xa500;
            if(TEST_CASE==21) {s->result=-110;s->valid=3;
                snapshot->clock.result=-19;snapshot->clock.valid=0;
                snapshot->usb.result=-19;snapshot->usb.valid=0;
                snapshot->wake.result=-19;snapshot->wake.written=0;}
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
        CHECK(prefix(last->rows[15],"BUILD: M2-USBACM-04"));
        if(TEST_CASE>=30 && TEST_CASE<=32) {
            CHECK(prefix(last->rows[3],TEST_CASE==32 ? "US:2 RC:-110 IRQ:0 D:--" : "US:2 RC:-16 IRQ:0 D:--"));
            CHECK(prefix(last->rows[4]+24,"CHR:---- D:?"));
            CHECK(prefix(last->rows[5],"POLL:1"));
            CHECK(prefix(last->rows[5]+10,TEST_CASE==32 ? "PW:-110 V:3" : "PW:-16 V:0"));
            CHECK(prefix(last->rows[10],TEST_CASE==30 ? "WACS:00200001>00200001" :
                TEST_CASE==31 ? "WACS:00360001>00360001" : "WACS:00360001>00340023"));
            CHECK(prefix(last->rows[11],"G M:00 W:01 A:000001FF C:01 I:01"));
            CHECK(prefix(last->rows[16],"WAKE:0"));
            CHECK(prefix(last->rows[17],"6A:04>00 PHY:00000002120000"));
            CHECK(prefix(last->rows[18],"CTL:1A:10 1D:00 22:00 63:00"));
            CHECK(prefix(last->rows[19],"TRIM:00:00/00 05:00/00 15:00/00"));
        }
        if(TEST_CASE==33) CHECK(prefix(last->rows[6],"USB UNPLUGGED / HEARTBEAT LIVE"));
        if(TEST_CASE==26) CHECK(prefix(last->rows[6],"ATTACH USB CABLE NOW"));
        if(TEST_CASE==27) CHECK(prefix(last->rows[6],"USB ENUMERATION IN PROGRESS"));
        if(TEST_CASE==28) {
            CHECK(prefix(last->rows[6],"USB CONFIGURED / HOST SENDS LOG1"));
            CHECK(prefix(last->rows[3],"US:6 RC:0 IRQ:42 D:98"));
        }
        if(TEST_CASE==29) {
            CHECK(prefix(last->rows[6],"USB STOP RC:-19"));
            CHECK(prefix(last->rows[3],"US:1 RC:-19 IRQ:0 D:--"));
            CHECK(prefix(last->rows[4]+24,"CHR:---- D:?"));
        }
        if(TEST_CASE==0) {
            CHECK(prefix(last->rows[2],"LINUX: 6.18.0-y2-m2-usbacm4"));
            CHECK(prefix(last->rows[3],"USB RC:0"));
            CHECK(prefix(last->rows[3]+12,"VALID:001FFFFF"));
            CHECK(prefix(last->rows[4],"PW:0"));
            CHECK(prefix(last->rows[4]+8,"/7"));
            CHECK(prefix(last->rows[4]+24,"CHR:A520 D:1"));
            CHECK(prefix(last->rows[4]+14,"CLK:0/15"));
            CHECK(prefix(last->rows[16],"WAKE:0"));
            CHECK(prefix(last->rows[16]+9,"W:1 V:7F/FFF P:20 D:80"));
            CHECK(prefix(last->rows[17],"6A:04>00 PHY:00000002120000"));
            CHECK(prefix(last->rows[18],"CTL:1A:10 1D:00 22:00 63:00"));
            CHECK(prefix(last->rows[19],"TRIM:00:00/00 05:00/00 15:00/00"));
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
        if(TEST_CASE==13) CHECK(prefix(last->rows[4]+14,"CLK:-16/1"));
        if(TEST_CASE==14) CHECK(prefix(last->rows[3],"USB RC:-16"));
        if(TEST_CASE==15) {
            CHECK(prefix(last->rows[3],"USB RC:-19"));
            CHECK(prefix(last->rows[16],"PERI:00000400"));
            CHECK(prefix(last->rows[17],"MUX:01010100"));
        }
        if(TEST_CASE==16) {
            CHECK(prefix(last->rows[3],"USB RC:-5"));
            CHECK(prefix(last->rows[3]+12,"VALID:00000001"));
            CHECK(prefix(last->rows[16],"PRE P:20 D:-- HW:----"));
            CHECK(prefix(last->rows[17],"PHY68:-- -- -- -- -- -- --"));
            CHECK(prefix(last->rows[18],"DMA:--------------------------------"));
            CHECK(prefix(last->rows[19],"IRQE TX:---- RX:---- USB:--"));
        }
        if(TEST_CASE==17) {
            CHECK(prefix(last->rows[16],"WAKE:-19"));
            CHECK(prefix(last->rows[16]+9,"W:0 V:7F/000 P:-- D:--"));
            CHECK(prefix(last->rows[17],"6A:04>-- PHY:--------------"));
            CHECK(prefix(last->rows[19],"TRIM:00:00/-- 05:00/-- 15:00/--"));
        }
        if(TEST_CASE==18) CHECK(prefix(last->rows[16],"WAKE:-5"));
        if(TEST_CASE==19) CHECK(prefix(last->rows[16],"WAKE:-16"));
        if(TEST_CASE==20) CHECK(prefix(last->rows[4]+24,"CHR:A500 D:0"));
        if(TEST_CASE==21) {
            CHECK(prefix(last->rows[4],"PW:-110"));
            CHECK(prefix(last->rows[4]+8,"/3"));
            CHECK(prefix(last->rows[4]+24,"CHR:---- D:?"));
            CHECK(prefix(last->rows[12],"LAST ERR: PWRAP -110"));
        }
        if(TEST_CASE==22 || TEST_CASE==25) {
            CHECK(prefix(last->rows[16],TEST_CASE==22 ?
                "PRE P:20 D:80 HW:1900 W:-19/0" : "PRE P:20 D:80 HW:1900 W:-12/0"));
            CHECK(prefix(last->rows[17],"PHY68:06 07 BE 09 0A 0B 0C"));
            CHECK(prefix(last->rows[18],"DMA:ABCD0000000000000000000000001234"));
            CHECK(prefix(last->rows[19],"IRQE TX:1234 RX:5678 USB:9A"));
            CHECK(prefix(last->rows[12],TEST_CASE==22 ?
                "LAST ERR: PHY WAKE -19" : "LAST ERR: PHY WAKE -12"));
        }
        if(TEST_CASE==23) {
            CHECK(prefix(last->rows[16]+9,"W:0 V:07/000 P:-- D:--"));
            CHECK(prefix(last->rows[18],"CTL:1A:-- 1D:-- 22:-- 63:--"));
        }
        if(TEST_CASE==24) {
            CHECK(prefix(last->rows[16]+9,"W:1 V:7F/003 P:-- D:--"));
            CHECK(prefix(last->rows[17],"6A:04>-- PHY:0000----------"));
        }
        if(TEST_CASE==11) {
            CHECK(prefix(last->rows[4],"PW:-110"));
            CHECK(prefix(last->rows[4]+8,"/1"));
        }
        finish(0);
    }
    CHECK(0);return -38;
}
