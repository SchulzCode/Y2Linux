// SPDX-License-Identifier: GPL-2.0-only
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
int main(void)
{
	unsigned char factory[576]; struct stat st;
	const char *names[]={"WIFI","BT"}; unsigned lengths[]={512,64};
	if (geteuid()) return 1;
	int dir=open("/run/y2/factory",O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
	if (dir<0 || fstat(dir,&st) || st.st_uid || (st.st_mode&077)) return 1;
	for (unsigned i=0,at=0;i<2;at+=lengths[i++]) {
		int fd=openat(dir,names[i],O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK);
		if (fd<0 || fstat(fd,&st) || !S_ISREG(st.st_mode) || st.st_uid ||
		    (st.st_mode&077) || st.st_size!=lengths[i] || read(fd,factory+at,lengths[i])!=(ssize_t)lengths[i]) return 1;
		close(fd);
	}
	close(dir);
	int fd=open("/sys/devices/platform/18070000.connectivity/factory",O_WRONLY|O_CLOEXEC|O_NOFOLLOW);
	if (fd<0 || write(fd,factory,sizeof(factory))!=sizeof(factory)) return 1;
	explicit_bzero(factory,sizeof(factory)); close(fd);
	fd=open("/sys/devices/platform/18070000.connectivity/activate",O_WRONLY|O_CLOEXEC|O_NOFOLLOW);
	if (fd<0 || write(fd,"1\n",2)!=2) return 1;
	close(fd); return 0;
}
