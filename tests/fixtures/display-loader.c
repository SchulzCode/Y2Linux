/* Actual PID1 display orchestration under faulting syscall fixtures. */
typedef unsigned int u32;
static struct { long tty,kmsg,result;unsigned active,tail; } relay;
static unsigned nr_calls[400], aff_mask, close_count, finit_count;
static long fork_result=73,wait_result,aff_result,open_result=7,finit_result;
static char output[8192];static unsigned used;
static void fail(int line);
#define CHECK(x) do { if(!(x)) fail(__LINE__); } while(0)
static void relay_log(const char *p,unsigned n) { CHECK(used+n<sizeof(output));while(n--)output[used++]=*p++; }
static long call5(long nr,long a,long b,long c,long d,long e) {
 (void)d;(void)e;CHECK(nr>=0 && nr<400);nr_calls[nr]++;
 if(nr==241) {CHECK(!a && b==4);aff_mask=*(unsigned *)c;return aff_result;}
 if(nr==120) {CHECK(a==17 && !b && !c);return fork_result;}
 if(nr==114) {CHECK(a==73 && c==1 && !d);*(int *)b=0;return wait_result;}
 if(nr==5) {CHECK(b==0 || b==1);return open_result;}
 if(nr==6) {close_count++;return 0;}
 if(nr==4) return c;
 if(nr==379) {CHECK(a==7 && !*(char *)b && !c);finit_count++;return finit_result;}
 CHECK(0);return -1;
}
#include "../../initramfs/display.h"
static void reset(void) {
 display.pid=0;display.attempted=display.started=display.pending=0;
 relay.active=relay.tail=0;relay.tty=3;relay.kmsg=4;relay.result=0;
 for(unsigned i=0;i<400;++i) nr_calls[i]=0;
 fork_result=73;wait_result=aff_result=finit_result=0;open_result=7;
 aff_mask=close_count=finit_count=used=0;
}
static int test(void) {
 reset();display_service(15);CHECK(!display.attempted && !nr_calls[120]);
 relay.active=1;display_service(15);CHECK(!display.attempted);
 relay.tail=20;display_service(12);CHECK(!display.attempted && !nr_calls[120]);
 display_service(15);
 CHECK(display.pid==73 && nr_calls[120]==1 && nr_calls[114]==1 && aff_mask==1);
 CHECK(!nr_calls[379]); /* PID1 never loads synchronously */
 for(unsigned i=16;i<80;++i) display_service(i);
 CHECK(nr_calls[120]==1 && display.pending==60 && !finit_count);
 wait_result=73;display_service(80);CHECK(!display.pid);
 display_service(81);CHECK(nr_calls[120]==1);
 reset();relay.active=relay.tail=1;fork_result=-12;display_service(20);
 CHECK(display.attempted && !display.pid && !nr_calls[114]);
 reset();relay.active=relay.tail=1;aff_result=-22;display_service(20);
 CHECK(display.attempted && !nr_calls[120]);
 reset();relay.active=relay.tail=1;display_service(240);CHECK(!nr_calls[120]);
 reset();relay.active=relay.tail=1;display_service(20);wait_result=-4;
 display_service(21);CHECK(display.pid==73);wait_result=-10;display_service(22);CHECK(!display.pid);
 reset();CHECK(display_child()==0 && finit_count==1 && aff_mask==2 && close_count==4);
 reset();finit_result=-8;CHECK(display_child()==-8 && finit_count==1);
 reset();open_result=-2;CHECK(display_child()==-2 && !finit_count && close_count==2);
 reset();aff_result=-22;CHECK(display_child()==-22 && !finit_count && !close_count);
 return 0;
}
#ifdef ARM
static __attribute__((noreturn)) void finish(int status) {
 register long r0 __asm__("r0")=status;register long r7 __asm__("r7")=1;
 __asm__ volatile("svc 0" : "+r"(r0) : "r"(r7) : "memory");for(;;) {}
}
static void fail(int line) {finish(line%254+1);}
void _start(void) {finish(test());}
#else
#include <stdio.h>
#include <stdlib.h>
static void fail(int line) {fprintf(stderr,"failed line %d\n",line);exit(1);}
int main(void) {return test();}
#endif
