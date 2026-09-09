/* SPDX-License-Identifier: GPL-2.0-only
 * Y2B-245: freestanding PID1, bounded screen diagnostics; no runtime UART I/O.
 */
#include "status.h"
#include "../kernel/diagnostic/usb_state.h"
typedef unsigned int u32;
#ifdef Y2_SYSCALL_TEST
extern long y2_test_call(long,long,long,long,long,long);
#endif
static long call5(long number,long a,long b,long c,long d,long e)
{
#ifdef Y2_SYSCALL_TEST
    return y2_test_call(number,a,b,c,d,e);
#else
    register long r0 __asm__("r0")=a;
    register long r1 __asm__("r1")=b;
    register long r2 __asm__("r2")=c;
    register long r3 __asm__("r3")=d;
    register long r4 __asm__("r4")=e;
    register long r7 __asm__("r7")=number;
    __asm__ volatile("svc 0" : "+r"(r0) : "r"(r1),"r"(r2),"r"(r3),"r"(r4),"r"(r7) : "memory","cc");
    return r0;
#endif
}
static struct y2_text_frame screen;
static long diagnostic=-1,previous_draw=-1;
static unsigned beat,frames;
static char input[4097],value[160];
static void error(const char *where,long code)
{
    unsigned col;
    row_clear(screen.rows[12]);col=row_add(screen.rows[12],0,"LAST ERR: ");
    col=row_add(screen.rows[12],col,where);col=row_add(screen.rows[12],col," ");
    row_number(screen.rows[12],col,code);
}
static void present(const char *stage)
{
    unsigned col;
    row_clear(screen.rows[0]);col=row_add(screen.rows[0],0,"PID1:1 BEAT:");
    col=row_number(screen.rows[0],col,beat);
    col=row_add(screen.rows[0],col,beat&1 ? " [*] FRAME:" : " [ ] FRAME:");
    row_number(screen.rows[0],col,frames++);
    row_pair(screen.rows[1],"STAGE: ",stage);
    row_code(screen.rows[14],"PREVIOUS FRAME WRITE: ",previous_draw);
    previous_draw=call5(4,diagnostic,(long)&screen,sizeof(screen),0,0);
    if(previous_draw!=(long)sizeof(screen)) error("DRAW",previous_draw);
}
/* At most eight reads and 4096 bytes, nonblocking. Overflow is explicit. */
static long read_file(const char *path)
{
    unsigned used=0,attempt;
    long result=0,fd=call5(5,(long)path,2048,0,0,0);
    if(fd<0) { input[0]=0;return fd; }
    for(attempt=0;attempt<8;++attempt) {
        long n=call5(3,fd,(long)(input+used),4096-used,0,0);
        if(n==-4) continue;
        if(n<0) { result=n;break; }
        if(!n) { result=used;break; }
        used+=(unsigned)n;
        if(used>=4096) { result=-75;break; }
    }
    if(attempt==8) result=-11;
    input[used]=0;
    { long closed=call5(6,fd,0,0,0,0); if(closed<0 && result>=0) result=closed; }
    return result;
}
static void file_row(unsigned row,const char *key,const char *path,const char *field_key)
{
    long code=read_file(path);
    if(code==0) code=-61;
    if(code>=0 && field_key) code=field(value,sizeof(value),input,field_key);
    if(code<0) { row_code(screen.rows[row],key,code);error(key,code); }
    else {
        char *text=field_key ? value : input;
        unsigned i=0;while(text[i] && text[i]!='\n' && text[i]!='\r') ++i;
        text[i]=0;row_pair(screen.rows[row],key,text);
    }
}
static void timer_row(void)
{
    long code=read_file("/proc/interrupts");
    if(code>=0) code=irq_count(value,sizeof(value),input);
    if(code<0) { row_code(screen.rows[9],"TIMER IRQ ERR: ",code);error("TIMER",code); }
    else row_pair(screen.rows[9],"TIMER IRQ CPU0: ",value);
}
static void version_row(void)
{
    struct { char sysname[65],nodename[65],release[65],version[65],machine[65],domain[65]; } uts;
    long code=call5(122,(long)&uts,0,0,0,0);
    if(code<0) { row_code(screen.rows[2],"LINUX ERR: ",code);error("UNAME",code); }
    else { uts.release[64]=0;row_pair(screen.rows[2],"LINUX: ",uts.release); }
}
static void dt_rows(void)
{
    long n=read_file("/sys/firmware/devicetree/base/compatible");
    unsigned i;int found=0;
    for(i=0;n>0 && i+16<=(unsigned)n;++i)
        if(starts(input+i,"mediatek,mt6582") && input[i+15]==0) found=1;
    if(found) row_pair(screen.rows[4],"SOC: ","MT6582 (DT DESCRIBED)");
    else { row_code(screen.rows[4],"SOC DT ERR: ",n<0?n:-61);error("SOC DT",n<0?n:-61); }
    n=read_file("/sys/firmware/devicetree/base/memory@80000000/reg");
    if(n==16 && be32((unsigned char*)input)==0x80000000 &&
       be32((unsigned char*)input+4)==0x01800000 &&
       be32((unsigned char*)input+8)==0x84000000 &&
       be32((unsigned char*)input+12)==0x00080000)
        row_pair(screen.rows[6],"DT RAM: ","24M + 512K (D08 STATIC)");
    else { row_code(screen.rows[6],"DT RAM ERR: ",n<0?n:-22);error("DT RAM",n<0?n:-22); }
}
static long sleep_one(void)
{
    struct { long sec,nsec; } delay={1,0},rest;
    long code=-4;unsigned attempts;
    for(attempts=0;attempts<8 && code==-4;++attempts) {
        code=call5(162,(long)&delay,(long)&rest,0,0,0);
        if(code==-4) delay=rest;
    }
    return code;
}
static unsigned row_hex(char *row, unsigned col, unsigned value)
{
    char text[9]; unsigned i;
    for (i=0;i<8;++i) text[i]="0123456789ABCDEF"[(value >> (28-4*i)) & 15];
    text[8]=0; return row_add(row,col,text);
}
static unsigned row_hex_width(char *row,unsigned col,unsigned value,unsigned digits)
{
    static const char hex[]="0123456789ABCDEF";
    char buf[9];
    for(unsigned i=0;i<digits;++i) buf[i]=hex[(value >> ((digits-1-i)*4))&15];
    buf[digits]=0;return row_add(row,col,buf);
}
static void power_rows(void)
{
    struct y2_platform_snapshot snapshot;
    struct y2_pwrap_snapshot *s=&snapshot.power;
    struct y2_usb_clock_snapshot *c=&snapshot.clock;
    unsigned col;
    long n=call5(3,diagnostic,(long)&snapshot,sizeof(snapshot),0,0);
    if(n!=(long)sizeof(snapshot) || s->magic!=Y2_PWRAP_MAGIC) {
        row_code(screen.rows[4],"PWRAP READ ERR: ",n<0?n:-5);
        error("PWRAP READ",n<0?n:-5); return;
    }
    row_code(screen.rows[4],"PW:",s->result);
    col=row_add(screen.rows[4],8,"/");row_number(screen.rows[4],col,s->valid);
    col=row_add(screen.rows[4],14,"CLK:");col=row_number(screen.rows[4],col,c->result);
    col=row_add(screen.rows[4],col,"/");row_number(screen.rows[4],col,c->valid);
    row_code(screen.rows[3],"USB RC:",snapshot.usb.result);
    col=row_add(screen.rows[3],12,"VALID:");row_hex(screen.rows[3],col,snapshot.usb.valid);
    for(unsigned row=16;row<20;++row) row_clear(screen.rows[row]);
    if(!y2_usb_state_ready(&snapshot)) {
        row_pair(screen.rows[16],"PERI:","");row_hex(screen.rows[16],5,c->peri);
        row_pair(screen.rows[17],"MUX:","");row_hex(screen.rows[17],4,c->mux);
        row_pair(screen.rows[18],"PLL:","");row_hex(screen.rows[18],4,c->pll);
        row_pair(screen.rows[19],"PWR:","");row_hex(screen.rows[19],4,c->pll_power);
    } else {
        const unsigned *v=snapshot.usb.values;
        col=row_add(screen.rows[16],0,"POWER:");col=row_hex_width(screen.rows[16],col,v[0],2);
        col=row_add(screen.rows[16],col," DEV:");col=row_hex_width(screen.rows[16],col,v[1],2);
        col=row_add(screen.rows[16],col," HW:");row_hex_width(screen.rows[16],col,v[2],4);
        col=row_add(screen.rows[17],0,"PHY68:");
        for(unsigned i=6;i<13;++i) {
            col=row_hex_width(screen.rows[17],col,v[i],2);
            col=row_add(screen.rows[17],col," ");
        }
        col=row_add(screen.rows[18],0,"DMA:");
        for(unsigned i=13;i<21;++i) col=row_hex_width(screen.rows[18],col,v[i],4);
        col=row_add(screen.rows[19],0,"IRQE TX:");col=row_hex_width(screen.rows[19],col,v[3],4);
        col=row_add(screen.rows[19],col," RX:");col=row_hex_width(screen.rows[19],col,v[4],4);
        col=row_add(screen.rows[19],col," USB:");row_hex_width(screen.rows[19],col,v[5],2);
    }
    if(s->result) error("PWRAP",s->result);
    else if(c->result) error("CLOCK",c->result);
    else if(snapshot.usb.result) error("USB",snapshot.usb.result);
}
__attribute__((noreturn)) void diag_start(u32 *stack)
{
    char **argv=(char**)(stack+1);
    unsigned row;long proc,sys;
    if(stack[0]==2 && starts(argv[1],"--selftest") && !argv[1][10]) {
        const char msg[]="Y2DIAG SELFTEST ARM EABI OK\n";
        call5(4,1,(long)msg,sizeof(msg)-1,0,0);call5(1,0,0,0,0,0);
        for(;;) {}
    }
    for(row=0;row<Y2_ROWS;++row) row_clear(screen.rows[row]);
    screen.magic=Y2_TEXT_MAGIC;
    row_pair(screen.rows[12],"LAST ERR: ","NONE");
    row_pair(screen.rows[15],"BUILD: ","M2-USBSTATE-01");
    row_pair(screen.rows[16],"TRUNCATED TEXT: ","~ ; ERRORS: -ERRNO");
    row_pair(screen.rows[17],"SCOPE: ","CPU0 / INITRAMFS ONLY");
    row_pair(screen.rows[18],"HOST LIMIT: ","60S FROM POWER-ON");
    row_pair(screen.rows[19],"END: ","MANUAL BOOTIMG RESTORE");
    if(call5(20,0,0,0,0,0)!=1) { call5(1,2,0,0,0,0);for(;;) {} }
    diagnostic=call5(5,(long)"/dev/y2diag",2,0,0,0);
    if(diagnostic<0) goto stop; /* Kernel WAITING screen remains; no unsafe fallback. */
    present("PID1 ENTER");
    proc=call5(21,(long)"proc",(long)"/proc",(long)"proc",14,0);
    row_code(screen.rows[10],"PROC MOUNT RC: ",proc);if(proc<0) error("MOUNT PROC",proc);
    present("PROC MOUNT DONE");
    sys=call5(21,(long)"sysfs",(long)"/sys",(long)"sysfs",15,0);
    row_code(screen.rows[11],"SYSFS MOUNT RC: ",sys);if(sys<0) error("MOUNT SYS",sys);
    present("SYSFS MOUNT DONE");
    /* Visible loop starts before optional collection; no UART writes here. */
    for(beat=0;beat<50;) {
        long slept;
        present("WAIT 1S / LOOP LIVE");
        slept=sleep_one();row_code(screen.rows[13],"SLEEP RC: ",slept);
        if(slept) { error("SLEEP",slept);present("STOP: SLEEP ERROR");goto stop; }
        ++beat;present("HEARTBEAT / COLLECT");
        if(beat==1) {
            present("READ VERSION");version_row();
            present("READ CPU");file_row(3,"CPU PART: ","/proc/cpuinfo","CPU part");
            present("READ ONLINE");file_row(5,"CPUS ONLINE: ","/sys/devices/system/cpu/online",0);
            present("READ DT");dt_rows();
            present("READ USB STATE");power_rows();
        }
        present("READ MEMORY");file_row(7,"MEMTOTAL: ","/proc/meminfo","MemTotal");
        present("READ UPTIME");file_row(8,"UPTIME/IDLE S: ","/proc/uptime",0);
        present("READ TIMER IRQ");timer_row();
        present("STATUS / LOOP LIVE");
    }
    present("STOP: RESTORE ANDROID");
stop:
    if(diagnostic>=0) call5(6,diagnostic,0,0,0,0);
    for(;;) call5(29,0,0,0,0,0); /* pause; no reset/retry or busy loop */
}
