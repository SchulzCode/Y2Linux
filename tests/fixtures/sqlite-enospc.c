/* SPDX-License-Identifier: GPL-2.0-only */
/* Test-only syscall fault injection. Never installed or preloaded on Y2. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static char target[PATH_MAX];
static int injected;

int y2_test_enospc(int directory_fd, int wal)
{
    char link[64], root[PATH_MAX];
    struct stat st;
    ssize_t length;
    if (directory_fd < 0) { target[0] = 0; return injected; }
    if (fstat(directory_fd, &st) || !S_ISDIR(st.st_mode) || st.st_uid != geteuid())
        return -1;
    snprintf(link, sizeof(link), "/proc/self/fd/%d", directory_fd);
    length = readlink(link, root, sizeof(root) - 1);
    if (length <= 0 || length >= PATH_MAX - 32) return -1;
    root[length] = 0;
    memcpy(target, root, length);
    strcpy(target + length, wal ? "/fault.sqlite-wal" : "/fault.sqlite");
    injected = 0;
    return 0;
}

static int fail_write(int fd)
{
    char link[64], path[PATH_MAX];
    ssize_t length;
    if (!target[0]) return 0;
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    length = readlink(link, path, sizeof(path) - 1);
    if (length < 0) return 0;
    path[length] = 0;
    if (strcmp(path, target)) return 0;
    injected++;
    errno = ENOSPC;
    return 1;
}

ssize_t pwrite(int fd, const void *buffer, size_t count, off_t offset)
{
    static ssize_t (*next)(int, const void *, size_t, off_t);
    if (fail_write(fd)) return -1;
    if (!next) next = dlsym(RTLD_NEXT, "pwrite");
    return next(fd, buffer, count, offset);
}

ssize_t pwrite64(int fd, const void *buffer, size_t count, off64_t offset)
{
    static ssize_t (*next)(int, const void *, size_t, off64_t);
    if (fail_write(fd)) return -1;
    if (!next) next = dlsym(RTLD_NEXT, "pwrite64");
    return next(fd, buffer, count, offset);
}
