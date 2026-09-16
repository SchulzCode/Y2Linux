// SPDX-License-Identifier: GPL-2.0-only
/* Read-only factory provider. Only bounded reads from the protected logical
 * disk ranges; ext4 is parsed from RAM copies. Any journal replay changes only
 * those RAM copies, which are sealed before record reads; no source is writable.
 * Runtime outputs contain secrets and live in root-only tmpfs. This process
 * never prints a radio address, raw record, or device-specific checksum.
 */
#define _GNU_SOURCE
#define _FILE_OFFSET_BITS 64
#include <errno.h>
#include <ext2fs/ext2fs.h>
#include <fcntl.h>
#include <linux/fs.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/random.h>
#include <sys/resource.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
#include "../../kernel/platform/connectivity/protocol.h"

#define NVRAM_BYTES 0x500000
#define PROTECT_BYTES 0xa00000
#define DISK_BYTES 7784103936ULL
static const char *runtime_dir = "/run/y2/factory";
static const char *data_dir = "/data/connectivity";
static const char *firmware_dir = "/lib/firmware/mediatek/mt6582";

static void fail(const char *reason)
{
	fprintf(stderr, "y2-factory: %s\n", reason);
	exit(EXIT_FAILURE);
}
static void read_exact(int fd, void *data, size_t size, off_t offset)
{
	unsigned char *p = data;
	while (size) {
		ssize_t n = pread(fd, p, size, offset);
		if (n < 0 && errno == EINTR) continue;
		if (n <= 0) fail("bounded factory read failed");
		p += n; offset += n; size -= n;
	}
}
static int factory_sysfs(const char *parent, const char *name, char *value, size_t capacity)
{
	char path[PATH_MAX]; ssize_t n;
	if (snprintf(path,sizeof(path),"%s/%s",parent,name) >= (int)sizeof(path)) return -1;
	int fd=open(path,O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
	if (fd<0) return -1;
	do { n=read(fd,value,capacity); } while (n<0 && errno==EINTR);
	close(fd);
	if (n<=0 || (size_t)n>=capacity) return -1;
	value[n]=0;
	return 0;
}
static int factory_parent(dev_t device, unsigned partition, char *parent)
{
	char link[64], number[16], expected[16], type[16], extra;
	unsigned index;
	snprintf(link,sizeof(link),"/sys/dev/block/%u:%u",major(device),minor(device));
	if (!realpath(link,parent) || factory_sysfs(parent,"partition",number,sizeof(number))) return -1;
	snprintf(expected,sizeof(expected),"%u\n",partition);
	if (strcmp(number,expected)) return -1;
	char *slash=strrchr(parent,'/');
	if (!slash) return -1;
	*slash=0;
	slash=strrchr(parent,'/');
	if (!slash || sscanf(slash+1,"mmcblk%u%c",&index,&extra)!=1 ||
	    factory_sysfs(parent,"device/type",type,sizeof(type)) || strcmp(type,"MMC\n")) return -1;
	return 0;
}
static int open_factory_disk(void)
{
	/* MMC probe order is not persistent. Identify the whole eMMC through
	 * mounted Y2ROOT/Y2DATA, then verify its node and geometry before reads. */
	char parent[PATH_MAX], data_parent[PATH_MAX], device[64], node[64], newline, extra;
	struct stat root, data, disk;
	unsigned dev_major,dev_minor; uint64_t size;
	if (stat("/",&root) || stat("/data",&data) ||
	    factory_parent(root.st_dev,5,parent) || factory_parent(data.st_dev,7,data_parent) ||
	    strcmp(parent,data_parent) || factory_sysfs(parent,"dev",device,sizeof(device)) ||
	    sscanf(device,"%u:%u%c%c",&dev_major,&dev_minor,&newline,&extra)!=3 || newline!='\n') return -1;
	if (snprintf(node,sizeof(node),"/dev/%s",strrchr(parent,'/')+1) >= (int)sizeof(node)) return -1;
	int fd=open(node,O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
	if (fd<0) return -1;
	if (fstat(fd,&disk) || !S_ISBLK(disk.st_mode) || disk.st_rdev!=makedev(dev_major,dev_minor) ||
	    ioctl(fd,BLKGETSIZE64,&size) || size!=DISK_BYTES) {
		close(fd); return -1;
	}
	return fd;
}
static int private_dir(const char *path)
{
	struct stat st;
	if (mkdir(path, 0700) && errno != EEXIST) fail("private directory unavailable");
	int fd = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
	if (fd < 0 || fstat(fd, &st) || (st.st_mode & 077) || st.st_uid != geteuid())
		fail("private directory ownership/permissions");
	return fd;
}
static void write_private(int dir, const char *name, const void *data, size_t size)
{
	/* New private files only: no clobbering an address, key, symlink or state. */
	int fd = openat(dir, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
	if (fd < 0) fail("private output already exists or cannot be created");
	const unsigned char *p = data;
	while (size) {
		ssize_t n = write(fd, p, size);
		if (n < 0 && errno == EINTR) continue;
		if (n <= 0) fail("private output write failed");
		p += n; size -= n;
	}
	if (fsync(fd) || close(fd) || fsync(dir)) fail("private output sync failed");
}
static int valid_address(const unsigned char *p)
{
	unsigned any = 0;
	for (unsigned i = 0; i < 6; i++) any |= p[i];
	return any && !(p[0] & 1);
}
static void persistent_address(int dir, const char *name, unsigned char *p)
{
	struct stat st;
	int fd = openat(dir, name, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
	if (fd >= 0) {
		if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size != 6 ||
		    st.st_uid != geteuid() || (st.st_mode & 077)) fail("persistent identity permissions/size");
		read_exact(fd, p, 6, 0); close(fd);
		if (!valid_address(p)) fail("persistent identity invalid; refusing replacement");
		return;
	}
	if (errno != ENOENT) fail("persistent identity read failed");
	/* A stable locally administered unicast identity, independently generated
	 * for each radio. getrandom blocks until the kernel RNG is initialized. */
	size_t left = 6;
	while (left) {
		ssize_t n = getrandom(p + 6 - left, left, 0);
		if (n < 0 && errno == EINTR) continue;
		if (n <= 0) fail("kernel random identity unavailable");
		left -= n;
	}
	p[0] = (p[0] & 0xfe) | 2;
	write_private(dir, name, p, 6);
}
static void template_read(const char *name, unsigned char *data, unsigned size)
{
	char path[256]; struct stat st;
	if (snprintf(path, sizeof(path), "%s/%s", firmware_dir, name) >= (int)sizeof(path))
		fail("firmware path too long");
	int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
	if (fd < 0 || fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size != size)
		fail("provisioned stock board defaults unavailable");
	read_exact(fd, data, size, 0); close(fd);
}
static int nvram_empty(const unsigned char *data)
{
	/* Stock libnvram NVM_HistoryLog: 512-byte records at 4 MiB; they are
	 * recovery receipts, not a radio-calibration container. Main backup bytes
	 * must really be empty before selecting stock board defaults. */
	for (unsigned i = 0; i < 0x400000; i++) if (data[i]) return 0;
	for (unsigned i = 0x400000; i < NVRAM_BYTES; i += 512) {
		unsigned any = 0;
		for (unsigned j = 0; j < 512; j++) any |= data[i + j];
		if (any && y2_conn_le32(data + i) != 0x5a5a7b7b) return 0;
	}
	return 1;
}
static void replay_ram_journal(int memory)
{
	/* Stock left the filesystem mounted when this retained copy was taken.
	 * Replay only its journal, only in bounded anonymous RAM. The original
	 * eMMC descriptor is O_RDONLY and closed in the child by CLOEXEC. */
	if (fcntl(memory, F_ADD_SEALS, F_SEAL_GROW | F_SEAL_SHRINK)) fail("factory RAM bounds");
	pid_t child = fork();
	if (child < 0) fail("journal worker unavailable");
	if (!child) {
		char path[64]; snprintf(path, sizeof(path), "/proc/self/fd/%d", memory);
		if (fcntl(memory, F_SETFD, 0)) _exit(126);
		int null = open("/dev/null", O_RDWR);
		if (null < 0 || dup2(null, 1) < 0 || dup2(null, 2) < 0) _exit(126);
		execl("/sbin/e2fsck", "e2fsck", "-p", "-E", "journal_only", path, (char *)NULL);
		_exit(127);
	}
	int status = 0;
	for (unsigned i = 0; i < 300; i++) {
		pid_t result = waitpid(child, &status, WNOHANG);
		if (result == child) {
			if (!WIFEXITED(status) || WEXITSTATUS(status) > 1) fail("RAM journal replay failed");
			return;
		}
		if (result < 0 && errno != EINTR) fail("journal worker wait failed");
		struct timespec delay = {.tv_nsec = 100000000}; nanosleep(&delay, NULL);
	}
	kill(child, SIGKILL); waitpid(child, &status, 0);
	fail("RAM journal replay deadline");
}
static void protect_records(const unsigned char *data, int output, unsigned which)
{
	/* Only this product's MD record names are admitted. No recursive export
	 * of private directories. The buffers remain inaccessible to other users. */
	const char *names[2] = { which ? "md/ST33B004" : "md/ST33A004", "md/MP0D_000" };
	unsigned sizes[2] = {2060, 4};
	int memory = memfd_create("y2-factory-ro", MFD_CLOEXEC | MFD_ALLOW_SEALING);
	if (memory < 0 || ftruncate(memory, PROTECT_BYTES)) fail("factory RAM allocation");
	size_t at = 0;
	while (at < PROTECT_BYTES) {
		ssize_t n = pwrite(memory, data + at, PROTECT_BYTES - at, at);
		if (n < 0 && errno == EINTR) continue;
		if (n <= 0) fail("factory RAM copy");
		at += n;
	}
	/* Superblock feature-incompat at 1024+0x60; journal replay changes RAM. */
	if (y2_conn_le32(data + 1024 + 0x60) & EXT3_FEATURE_INCOMPAT_RECOVER)
		replay_ram_journal(memory);
	if (fcntl(memory, F_ADD_SEALS, F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL))
		fail("factory RAM sealing");
	char path[64]; snprintf(path, sizeof(path), "/proc/self/fd/%d", memory);
	ext2_filsys fs;
	if (ext2fs_open(path, EXT2_FLAG_64BITS, 0, 0, unix_io_manager, &fs))
		fail("protected filesystem cannot be read without repair");
	if (fs->super->s_feature_incompat & EXT3_FEATURE_INCOMPAT_RECOVER)
		fail("protected filesystem needs recovery; refusing journal replay");
	for (unsigned i = 0; i < (which ? 1U : 2U); i++) {
		ext2_ino_t inode; struct ext2_inode info; ext2_file_t file;
		unsigned char record[2060]; unsigned got = 0;
		if (ext2fs_namei(fs, EXT2_ROOT_INO, EXT2_ROOT_INO, names[i], &inode) ||
		    ext2fs_read_inode(fs, inode, &info) || !LINUX_S_ISREG(info.i_mode) ||
		    EXT2_I_SIZE(&info) != sizes[i]) fail("protected record mapping/size mismatch");
		if (ext2fs_file_open(fs, inode, 0, &file) ||
		    ext2fs_file_read(file, record, sizes[i], &got) || got != sizes[i])
			fail("protected record read failed");
		ext2fs_file_close(file);
		write_private(output, names[i] + 3, record, sizes[i]);
		explicit_bzero(record, sizeof(record));
	}
	ext2fs_close(fs); close(memory);
}
int main(int argc, char **argv)
{
	const char *images = NULL;
#ifdef Y2_FACTORY_HOST_TEST
	if (argc != 5) fail("host usage: images runtime data provisioned-firmware");
	images = argv[1]; runtime_dir = argv[2]; data_dir = argv[3]; firmware_dir = argv[4];
#else
	(void)argv;
	if (argc != 1 || geteuid()) fail("production factory provider requires root and no arguments");
#endif
	int output = private_dir(runtime_dir), persistent = private_dir(data_dir);
	struct rlimit core={0,0};
	if (setrlimit(RLIMIT_CORE,&core) || prctl(PR_SET_DUMPABLE,0)) fail("private memory protection");
	/* A previous incomplete provider run has no published consumers. Retry
	 * only its fixed private tmpfs outputs; persistent identities never change. */
	struct stat previous;
	if (fstatat(output,"ready",&previous,AT_SYMLINK_NOFOLLOW)==0) fail("provider already published");
	if (errno!=ENOENT) fail("provider readiness check");
	const char *partial[]={"ST33A004","ST33B004","MP0D_000","WIFI","BT"};
	for (unsigned i=0;i<sizeof(partial)/sizeof(partial[0]);i++) {
		if (fstatat(output,partial[i],&previous,AT_SYMLINK_NOFOLLOW)) {
			if (errno==ENOENT) continue;
			fail("partial output check");
		}
		if (!S_ISREG(previous.st_mode) || previous.st_uid!=geteuid() || (previous.st_mode&077) ||
		    unlinkat(output,partial[i],0)) fail("invalid partial output");
	}
	unsigned char *nvram = malloc(NVRAM_BYTES), *protect = malloc(PROTECT_BYTES);
	unsigned char wifi[512], bt[64];
	if (!nvram || !protect) fail("factory RAM allocation");
	int disk = -1;
	if (!images) {
		disk = open_factory_disk();
		if (disk < 0) fail("mounted production eMMC identity/geometry mismatch");
		read_exact(disk, nvram, NVRAM_BYTES, 0x400000);
	} else {
		char path[512]; snprintf(path, sizeof(path), "%s/NVRAM.bin", images);
		int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
		if (fd < 0) fail("retained NVRAM unavailable");
		read_exact(fd, nvram, NVRAM_BYTES, 0); close(fd);
	}
	/* A nonempty backup needs its verified AllMap decoder; never silently
	 * discard unknown factory calibration or overwrite it with defaults. */
	if (!nvram_empty(nvram)) fail("nonempty/unknown factory backup: records must be mapped first");
	explicit_bzero(nvram, NVRAM_BYTES); free(nvram);
	for (unsigned which = 0; which < 2; which++) {
		if (images) {
			char path[512]; snprintf(path, sizeof(path), "%s/PROTECT_%c.bin", images, which ? 'S' : 'F');
			int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
			if (fd < 0) fail("retained protected filesystem unavailable");
			read_exact(fd, protect, PROTECT_BYTES, 0); close(fd);
		} else read_exact(disk, protect, PROTECT_BYTES, which ? 0x1300000 : 0x900000);
		protect_records(protect, output, which);
	}
	if (disk >= 0) close(disk);
	explicit_bzero(protect, PROTECT_BYTES); free(protect);
	template_read("WIFI.defaults", wifi, sizeof(wifi));
	template_read("BT.defaults", bt, sizeof(bt));
	if (y2_conn_le16(wifi) != 0x104 || wifi[196] != 1 || wifi[197]) fail("board Wi-Fi default mismatch");
	static const unsigned char zero[6]={0};
	if (memcmp(wifi+4,zero,6) || memcmp(bt,zero,6)) fail("board template contains an address");
	persistent_address(persistent, "wifi-address", wifi + 4);
	persistent_address(persistent, "bluetooth-address", bt);
	if (!memcmp(wifi + 4, bt, 6)) fail("radio identities collide");
	write_private(output, "WIFI", wifi, sizeof(wifi));
	write_private(output, "BT", bt, sizeof(bt));
	/* Publish completion last. Consumers must reject a partial provider run. */
	static const char status[] = "version=1\nfactory_backup=empty\nrf_parameters=stock_board_defaults\nidentity=persistent_fallback\n";
	write_private(output, "ready", status, sizeof(status) - 1);
	explicit_bzero(wifi, sizeof(wifi)); explicit_bzero(bt, sizeof(bt));
	close(output); close(persistent);
	puts("y2-factory: private records ready; factory partitions were read-only");
	return 0;
}
