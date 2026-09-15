// SPDX-License-Identifier: GPL-2.0-only
/* The only userspace part of calibration implements bounded filesystem
 * semantics. The kernel owns MD1 power, memory, mailbox, deadlines and stop.
 * A service restart cannot silently discard an in-flight calibration. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>
#include "fs-store.h"
#include "../../kernel/platform/connectivity/calibration.h"

static int seed(struct y2_store *store, int factory)
{
	static const char *names[] = {"ST33A004","ST33B004","MP0D_000"};
	static const char *paths[] = {"X/ST33A004","Y/ST33B004","X/MP0D_000"};
	static const unsigned sizes[] = {2060,2060,4};
	const char *roots[] = {"Z/","X/","Y/","W/"};
	for (unsigned i=0;i<4;i++) if (!y2_fs_new(store,roots[i],1)) return -1;
	for (unsigned i=0;i<3;i++) {
		unsigned char data[2060]; struct stat st;
		int fd=openat(factory,names[i],O_RDONLY|O_CLOEXEC|O_NOFOLLOW|O_NONBLOCK);
		if (fd < 0) return -1;
		int ret=fstat(fd,&st) || !S_ISREG(st.st_mode) || st.st_size != sizes[i] ||
			st.st_uid || (st.st_mode & 077) || read(fd,data,sizes[i]) != (ssize_t)sizes[i];
		close(fd);
		if (!ret) ret=y2_fs_seed(store,paths[i],data,sizes[i]);
		explicit_bzero(data,sizeof(data)); if (ret) return -1;
	}
	return 0;
}
int main(void)
{
	struct rlimit core={0,0}, memory={64*1024*1024,64*1024*1024};
	struct y2_store *store=calloc(1,sizeof(*store));
	unsigned char input[Y2_CAL_MESSAGE], output[Y2_CAL_MESSAGE];
	unsigned generation=0, serial=0, served=0;
	if (geteuid() || !store || setrlimit(RLIMIT_CORE,&core) ||
	    setrlimit(RLIMIT_AS,&memory) || prctl(PR_SET_DUMPABLE,0) ||
	    prctl(PR_SET_NO_NEW_PRIVS,1,0,0,0)) return 1;
	int factory=open("/run/y2/factory",O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW);
	int radio=open("/dev/y2-calibration",O_RDWR|O_CLOEXEC|O_NOFOLLOW);
	if (factory<0 || radio<0) { fputs("y2-calibration: provider or kernel unavailable\n",stderr); return 1; }
    int ready=open("/run/y2/calibration-ready",O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,0600);
    if (ready<0 || write(ready,"1\n",2)!=2 || close(ready)) return 1;
	for (;;) {
		ssize_t n=read(radio,input,sizeof(input));
		if (n<0 && (errno==EINTR || errno==EAGAIN)) continue;
		if (n<Y2_CAL_HEADER+8 || n>Y2_CAL_MESSAGE || y2_conn_le32(input)!=Y2_CAL_VERSION ||
		    y2_conn_le32(input+12)!=(unsigned)n-Y2_CAL_HEADER) break;
		unsigned next_generation=y2_conn_le32(input+4), next_serial=y2_conn_le32(input+8);
		if (generation!=next_generation) {
			if (!next_generation || next_serial!=1) break;
			y2_fs_clear(store);
			if (seed(store,factory)) break;
			generation=next_generation; serial=served=0;
		}
		if (next_serial!=serial+1 || served>=4096) break;
		int result=y2_fs_dispatch(store,input+Y2_CAL_HEADER,n-Y2_CAL_HEADER,output+Y2_CAL_HEADER);
		if (result<8 || result>Y2_FS_STRIDE) break;
		memcpy(output,input,Y2_CAL_HEADER); y2_conn_put32(output+12,result);
		ssize_t written;
		do { written=write(radio,output,result+Y2_CAL_HEADER); } while(written<0 && errno==EINTR);
		explicit_bzero(input,sizeof(input)); explicit_bzero(output,sizeof(output));
		if (written!=result+Y2_CAL_HEADER) break;
		serial=next_serial; served++;
	}
	/* Aggregate counters only; never names, identifiers, data or checksums. */
	fprintf(stderr,"y2-calibration: stopped; requests=%u RAM=%u shadow-writes=%u\n",
		served,store->used,store->shadow_writes);
	y2_fs_clear(store); free(store); close(radio); close(factory);
	return 1;
}
