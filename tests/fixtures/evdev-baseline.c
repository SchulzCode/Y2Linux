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
#include "../../initramfs/status.h"
static unsigned relay_length(const char*s){unsigned n=0;while(s[n])++n;return n;}
static unsigned logged,late,mode,read_count[8];
static long call5(long,long,long,long,long,long);
static void relay_log(const char*s,unsigned n){(void)s;logged+=n;}
static int relay_device(const char*b,unsigned n,unsigned*d)
{if(n!=5||b[0]!='1'||b[1]!='3'||b[2]!=':')return 0;*d=(13<<8)|(b[3]-'0');return 1;}
#include "../../initramfs/evdev.h"
static const char*names[]={"Y2 navigation buttons","APT32F click-wheel","mt6582-keypad","mtk-pmic-keys"};
static void copy(char*dst,const char*src,unsigned n){for(unsigned i=0;i<n;++i)dst[i]=src[i];}
static long call5(long op,long a,long b,long c,long d,long e)
{
 (void)d;(void)e;
 if(op==5){const char*p=(void*)a;
  if(p[1]=='d')return 100+p[12]-'0';
  unsigned i=p[22]-'0';if(i>3||(!late&&i>1))return -2;
  return (relay_length(p)>30?10:20)+i;
 }
 if(op==14||op==6)return 0;
 if(op==54){unsigned i=a-100;
  if((unsigned)b==0x80804506){unsigned n=relay_length(names[i])+1;copy((void*)c,names[i],n);return n;}
  CHECK((unsigned)b==0x80204518);for(unsigned j=0;j<32;++j)((unsigned char*)c)[j]=0;return 32;
 }
 CHECK(op==3);
 if(a>=10&&a<14){unsigned n=relay_length(names[a-10]);copy((void*)b,names[a-10],n);((char*)b)[n]='\n';return n+1;}
 if(a>=20&&a<24){char name[]="13:0\n";name[3]='0'+a-20;copy((void*)b,name,5);return 5;}
 CHECK(a>=100&&a<104);unsigned i=a-100;
 if(read_count[i]++)return -11;
 struct nav_event *v=(void*)b;
 *v=(struct nav_event){.type=mode?0:1,.code=mode?3:28,.value=1};
 return mode==2?15:16;
}
static int run(void)
{
 CHECK(nav_name("mt6582-keypad\n",14));CHECK(!nav_name("other\n",6));
 nav_open();CHECK(nav.opened==3);
 nav_service();CHECK(nav.events==2&&!nav.result);
 late=1;nav_service();CHECK(nav.opened==15&&nav.events==4);
 for(unsigned i=0;i<8;++i)read_count[i]=0;
 mode=1;nav_service();CHECK(nav.result==-75);
 for(unsigned i=0;i<8;++i)read_count[i]=0;
 mode=2;nav_service();CHECK(nav.result==-71);
 nav_close();CHECK(!nav.opened&&logged);return 0;
}
#ifdef ARM
void _start(void){finish(run());}
#else
int main(void){return run();}
#endif
