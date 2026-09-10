/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_EVDEV_H
#define Y2_EVDEV_H
/* ARM EABI evdev; discover all M2 devices by name, including deferred probes. */
struct nav_event { unsigned sec,usec; unsigned short type,code; int value; };
_Static_assert(sizeof(struct nav_event)==16,"ARM evdev ABI");
static struct {long fd[8],result;unsigned opened,sequence,events;} nav;
static void nav_log(const char*kind,long a,long b,long c)
{
 char line[Y2_COLS+1];unsigned col;
 row_clear(line);col=row_add(line,0,"INPUT ");col=row_add(line,col,kind);
 col=row_add(line,col," ");col=row_number(line,col,a);
 col=row_add(line,col," ");col=row_number(line,col,b);
 col=row_add(line,col," ");col=row_number(line,col,c);
 line[col++]='\n';relay_log(line,col);
}
static long nav_read_file(const char*path,char*buf,unsigned size)
{
 long fd=call5(5,(long)path,2048,0,0,0),n;
 if(fd<0)return fd;
 n=call5(3,fd,(long)buf,size,0,0);call5(6,fd,0,0,0,0);return n;
}
static int nav_name(const char*b,long n)
{
 static const char*names[]={"Y2 navigation buttons","APT32F click-wheel","mt6582-keypad","mtk-pmic-keys"};
 for(unsigned k=0;k<4;++k){unsigned l=relay_length(names[k]);if(n!=(long)l+1)continue;
  unsigned i=0;while(i<l&&b[i]==names[k][i])++i;
  if(i==l&&(b[l]=='\n'||!b[l]))return 1;
 }return 0;
}
static void nav_open(void)
{
 char np[]="/sys/class/input/event0/device/name",dp[]="/sys/class/input/event0/dev";
 char node[]="/dev/y2input0",buf[128];unsigned dev;long n;
 for(unsigned i=0;i<8;++i){
  if(nav.opened&(1U<<i))continue;
  np[22]=dp[22]='0'+i;node[12]='0'+i;
  n=nav_read_file(np,buf,sizeof(buf));if(!nav_name(buf,n))continue;
  relay_log("INPUT DEVICE ",13);relay_log(buf,(unsigned)n);
  n=nav_read_file(dp,buf,sizeof(buf));
  if(n<=0||n>=(long)sizeof(buf)||!relay_device(buf,n,&dev)){nav_log("DISCOVERY",i,n,0);continue;}
  n=call5(14,(long)node,0020400,dev,0,0);if(n<0&&n!=-17){nav_log("MKNOD",i,n,0);continue;}
  n=call5(5,(long)node,2048,0,0,0);if(n<0){nav_log("OPENFAIL",i,n,0);continue;}
  long fd=n;
  n=call5(54,fd,0x80804506,(long)buf,0,0); /* EVIOCGNAME(128) */
  if(!nav_name(buf,n)){nav_log("IDENTITY",i,n,0);call5(6,fd,0,0,0,0);continue;}
  nav.fd[i]=fd;nav.opened|=1U<<i;nav_log("OPEN",i,dev,0);
  unsigned char state[32];
  n=call5(54,fd,0x80204518,(long)state,0,0); /* EVIOCGKEY(32) */
  if(n<0)nav_log("STATE_ERR",i,n,0);
  else {static const unsigned keys[]={28,103,105,106,108,114,115,116,158,164};
   for(unsigned k=0;k<sizeof(keys)/sizeof(keys[0]);++k)
    if(keys[k]/8 < (unsigned)n && state[keys[k]/8]&(1U<<(keys[k]%8)))nav_log("INITIAL_DOWN",i,keys[k],1);
  }
 }
}
static void nav_close(void)
{for(unsigned i=0;i<8;++i)if(nav.opened&(1U<<i))call5(6,nav.fd[i],0,0,0,0);nav.opened=0;}
static void nav_service(void)
{
 struct nav_event events[32];
 nav_open();
 for(unsigned k=0;k<8;++k)if(nav.opened&(1U<<k)){
  for(unsigned batch=0;batch<4;++batch){
   long n=call5(3,nav.fd[k],(long)events,sizeof(events),0,0);
   if(n==-11)break;if(n==-4)continue;
   if(n<=0||n>(long)sizeof(events)||n%sizeof(events[0])){nav.result=n<0?n:-71;nav_log("ERROR",k,nav.result,0);break;}
   for(unsigned i=0;i<(unsigned)n/sizeof(events[0]);++i){
    struct nav_event*e=&events[i];++nav.sequence;
    if(e->type==0&&e->code==3){nav.result=-75;nav_log("DROPPED",k,nav.sequence,0);}
    else if(e->type==1){++nav.events;nav_log("KEY",k,e->code,e->value);}
   }
  }
 }
}
#endif
