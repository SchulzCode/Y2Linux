/* SPDX-License-Identifier: GPL-2.0-only
 * Root-only signed rescue updater. No BOOTIMG/partition-table write path.
 * Y2_UPDATE_TEST enables regular-file/fault fixtures; never used in the image.
 */
#define _GNU_SOURCE
#define _FILE_OFFSET_BITS 64
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <json-c/json.h>
#include <limits.h>
#include <linux/fs.h>
#include <sodium.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/ioctl.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/sysmacros.h>
#include <sys/types.h>
#include <unistd.h>
#include <zlib.h>

#define LIMIT (820ULL * 1024 * 1024)
#define RESERVE (96ULL * 1024 * 1024)
#define CHUNK 131072
static char error_text[192];
static int basefd = -1;
static const char *testroot;
static bool failure(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(error_text, sizeof(error_text), fmt, ap);
    va_end(ap);
    return false;
}
static const char *path(const char *p) {
    static char slots[8][PATH_MAX];
    static unsigned slot;
    char *out = slots[slot++ % 8];
    if (snprintf(out, PATH_MAX, "%s%s", testroot ? testroot : "", p) >= PATH_MAX)
        return "";
    return out;
}
static bool writeall(int fd, const void *buf, size_t n) {
    const char *p = buf;
    while (n) {
        ssize_t done = write(fd, p, n);
        if (done < 0 && errno == EINTR)
            continue;
        if (done <= 0)
            return failure("write: %s", strerror(errno));
        p += done;
        n -= (size_t)done;
    }
    return true;
}
static int openfile(int dir, const char *name, int flags, mode_t mode) {
    int fd = openat(dir, name, flags | O_CLOEXEC | O_NOFOLLOW, mode);
    struct stat st;
    if (fd < 0) {
        failure("open %s: %s", name, strerror(errno));
        return -1;
    }
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_nlink != 1) {
        close(fd);
        failure("not a private regular file: %s", name);
        return -1;
    }
    return fd;
}
static char *readfile(int dir, const char *name, size_t limit, size_t *len) {
    int fd = openfile(dir, name, O_RDONLY, 0);
    if (fd < 0)
        return NULL;
    struct stat st;
    if (fstat(fd, &st) || st.st_size < 0 || (uint64_t)st.st_size > limit) {
        close(fd);
        failure("file size: %s", name);
        return NULL;
    }
    char *buf = calloc((size_t)st.st_size + 1, 1);
    if (!buf) {
        close(fd);
        failure("allocation");
        return NULL;
    }
    size_t got = 0, need = (size_t)st.st_size;
    while (got < need) {
        ssize_t n = read(fd, buf + got, need - got);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0) {
            free(buf);
            close(fd);
            failure("short read: %s", name);
            return NULL;
        }
        got += (size_t)n;
    }
    close(fd);
    *len = got;
    return buf;
}
static json_object *parse(const char *buf, size_t n) {
    json_tokener *tok = json_tokener_new_ex(16);
    if (!tok)
        return NULL;
    json_tokener_set_flags(tok, JSON_TOKENER_STRICT | JSON_TOKENER_VALIDATE_UTF8);
    json_object *j = json_tokener_parse_ex(tok, buf, (int)n);
    bool ok = json_tokener_get_error(tok) == json_tokener_success &&
              json_tokener_get_parse_end(tok) == n && j && json_object_is_type(j, json_type_object);
    json_tokener_free(tok);
    if (!ok) {
        if (j)
            json_object_put(j);
        failure("invalid JSON object");
        return NULL;
    }
    return j;
}
static json_object *load(int dir, const char *name) {
    errno = 0;
    size_t n;
    char *buf = readfile(dir, name, 16384, &n);
    if (!buf)
        return NULL;
    json_object *j = parse(buf, n);
    free(buf);
    if (!j)
        errno = EINVAL;
    return j;
}
static json_object *field(json_object *j, const char *k) {
    json_object *v = NULL;
    json_object_object_get_ex(j, k, &v);
    return v;
}
static const char *str(json_object *j, const char *k) {
    json_object *v = field(j, k);
    return v && json_object_is_type(v, json_type_string) ? json_object_get_string(v) : "";
}
static int64_t num(json_object *j, const char *k) {
    json_object *v = field(j, k);
    return v && json_object_is_type(v, json_type_int) ? json_object_get_int64(v) : -1;
}
static bool yes(json_object *j, const char *k) {
    json_object *v = field(j, k);
    return v && json_object_is_type(v, json_type_boolean) && json_object_get_boolean(v);
}
static void setstr(json_object *j, const char *k, const char *v) {
    json_object_object_add(j, k, json_object_new_string(v));
}
static void setnum(json_object *j, const char *k, int64_t v) {
    json_object_object_add(j, k, json_object_new_int64(v));
}
static bool save(const char *name, json_object *j) {
    char tmp[80];
    if (snprintf(tmp, sizeof(tmp), ".%s.tmp", name) >= (int)sizeof(tmp))
        return failure("record name");
    /* An interrupted temp file can only belong to this locked writer. */
    if (unlinkat(basefd, tmp, 0) && errno != ENOENT)
        return failure("record cleanup");
    int fd = openfile(basefd, tmp, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0)
        return false;
    const char *s = json_object_to_json_string_ext(j, JSON_C_TO_STRING_PLAIN);
    bool ok = writeall(fd, s, strlen(s)) && fsync(fd) == 0;
    close(fd);
    if (!ok || renameat(basefd, tmp, basefd, name) || fsync(basefd))
        return failure("durable record: %s", strerror(errno));
    return true;
}
static bool hex(const char *s, size_t bytes, unsigned char *out) {
    if (strlen(s) != 2 * bytes)
        return false;
    for (const char *p = s; *p; p++)
        if (!(*p >= '0' && *p <= '9') && !(*p >= 'a' && *p <= 'f'))
            return false;
    return sodium_hex2bin(out, bytes, s, 2 * bytes, NULL, NULL, NULL) == 0;
}
static bool sha_match(const unsigned char *sum, const char *wanted) {
    unsigned char expected[32];
    return hex(wanted, 32, expected) && sodium_memcmp(sum, expected, 32) == 0;
}
static const char *hash_string(const unsigned char *sum) {
    static char out[65];
    sodium_bin2hex(out, sizeof(out), sum, 32);
    return out;
}
static bool enough(int fd, uint64_t bytes) {
#ifdef Y2_UPDATE_TEST
    if (getenv("Y2_TEST_FAULT") && !strcmp(getenv("Y2_TEST_FAULT"), "reserve"))
        return failure("data reserve unavailable");
#endif
    struct statvfs st;
    if (fstatvfs(fd, &st) || !st.f_frsize || (st.f_flag & ST_RDONLY) ||
        (uint64_t)st.f_bavail * st.f_frsize < RESERVE + bytes || (st.f_files && st.f_favail < 128))
        return failure("data reserve unavailable");
    return true;
}
static uint16_t u16(const unsigned char *p) { return (uint16_t)p[0] | (uint16_t)p[1] << 8; }
static uint32_t u32(const unsigned char *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static uint64_t root_bytes(const unsigned char *buf) {
    const unsigned char uuid[16] = {0x79, 0x32, 0x4c, 0x69, 0x6e, 0x75, 0x48, 0x01,
                                    0x80, 0,    0,    0,    0,    0,    1,    1};
    const unsigned char *s = buf + 1024;
    uint32_t log = u32(s + 24);
    if (u16(s + 56) != 0xef53 || memcmp(s + 104, uuid, 16) || memcmp(s + 120, "Y2ROOT\0", 7) ||
        log > 2)
        return 0;
    uint64_t blocks = u32(s + 4);
    if (u32(s + 96) & 0x80)
        blocks |= (uint64_t)u32(s + 336) << 32;
    uint64_t size = blocks * (1024ULL << log);
    return size >= 8 * 1024 * 1024 && size <= LIMIT ? size : 0;
}
/* gzip must have one member, exact encoded and raw lengths, hashes, ext4 identity.
 * The first pass has no destination; installation repeats validation while writing.
 */
static bool stream_image(int input, int output, json_object *payload, bool inject) {
    if (num(payload, "bytes") <= 0 || num(payload, "bytes") > (int64_t)LIMIT ||
        num(payload, "raw_bytes") < 8 * 1024 * 1024 || num(payload, "raw_bytes") > (int64_t)LIMIT)
        return failure("image metadata bounds");
    unsigned char in[CHUNK], out[CHUNK], header[4096], csum[32], rsum[32];
    size_t head = 0;
    uint64_t compressed = 0, raw = 0;
    crypto_hash_sha256_state ch, rh;
    crypto_hash_sha256_init(&ch);
    crypto_hash_sha256_init(&rh);
    z_stream z = {0};
    if (inflateInit2(&z, 15 + 16) != Z_OK)
        return failure("gzip init");
    if (lseek(input, 0, SEEK_SET) < 0 || (output >= 0 && lseek(output, 0, SEEK_SET) < 0)) {
        inflateEnd(&z);
        return failure("image seek");
    }
    bool ok = false, ended = false;
    for (;;) {
        ssize_t n = read(input, in, sizeof(in));
        if (n < 0 && errno == EINTR)
            continue;
        if (n < 0) {
            failure("payload read");
            break;
        }
        if (!n) {
            if (!ended)
                failure("incomplete gzip");
            else
                ok = true;
            break;
        }
        if (ended) {
            failure("trailing gzip bytes");
            break;
        }
        compressed += (uint64_t)n;
        if (compressed > (uint64_t)num(payload, "bytes")) {
            failure("encoded size");
            break;
        }
        crypto_hash_sha256_update(&ch, in, (size_t)n);
        z.next_in = in;
        z.avail_in = (uInt)n;
        do {
            z.next_out = out;
            z.avail_out = sizeof(out);
            int rc = inflate(&z, Z_NO_FLUSH);
            size_t count = sizeof(out) - z.avail_out;
            if (raw + count > (uint64_t)num(payload, "raw_bytes")) {
                failure("raw size");
                goto done;
            }
            if (head < sizeof(header)) {
                size_t copy = count < sizeof(header) - head ? count : sizeof(header) - head;
                memcpy(header + head, out, copy);
                head += copy;
            }
            raw += count;
            crypto_hash_sha256_update(&rh, out, count);
            if (head == sizeof(header) &&
                root_bytes(header) != (uint64_t)num(payload, "raw_bytes")) {
                failure("root ext4 identity/extent");
                goto done;
            }
            if (output >= 0 && count && !writeall(output, out, count))
                goto done;
#ifdef Y2_UPDATE_TEST
            if (inject && getenv("Y2_TEST_FAULT") &&
                !strcmp(getenv("Y2_TEST_FAULT"), "write_interrupt") && raw >= CHUNK)
                _exit(91);
#else
            (void)inject;
#endif
            if (rc == Z_STREAM_END) {
                ended = true;
                if (z.avail_in) {
                    failure("multiple gzip members");
                    goto done;
                }
                break;
            }
            if (rc == Z_BUF_ERROR && !z.avail_in && z.avail_out == sizeof(out))
                break;
            if (rc != Z_OK) {
                failure("gzip corruption");
                goto done;
            }
        } while (z.avail_in || z.avail_out == 0);
    }
    if (!ok)
        goto done;
    crypto_hash_sha256_final(&ch, csum);
    crypto_hash_sha256_final(&rh, rsum);
    ok = head == sizeof(header) && raw == (uint64_t)num(payload, "raw_bytes") &&
         compressed == (uint64_t)num(payload, "bytes") && sha_match(csum, str(payload, "sha256")) &&
         sha_match(rsum, str(payload, "raw_sha256"));
    if (!ok)
        failure("payload hash/size mismatch");
done:
    inflateEnd(&z);
    return ok;
}
static bool file_hash(int fd, uint64_t count, const char *expected) {
    if (lseek(fd, 0, SEEK_SET) < 0)
        return failure("readback seek");
    crypto_hash_sha256_state h;
    crypto_hash_sha256_init(&h);
    unsigned char buf[CHUNK], sum[32];
    while (count) {
        ssize_t n = read(fd, buf, count < sizeof(buf) ? (size_t)count : sizeof(buf));
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0)
            return failure("readback short read");
        crypto_hash_sha256_update(&h, buf, (size_t)n);
        count -= (uint64_t)n;
    }
    crypto_hash_sha256_final(&h, sum);
    return sha_match(sum, expected) || failure("readback hash mismatch");
}
static bool crypto_ready(void) {
    /* sodium_init may need the CSPRNG even for verification. Never weaken it. */
    unsigned char ready;
    return (getrandom(&ready, 1, GRND_NONBLOCK) == 1 && sodium_init() >= 0) ||
           failure("kernel entropy not ready");
}
static json_object *verify(int package, bool payload_check) {
    if (!crypto_ready())
        return NULL;
    size_t length, siglen;
    char *raw = readfile(package, "manifest.json", 16384, &length);
    if (!raw)
        return NULL;
    json_object *m = parse(raw, length), *trust = NULL, *compat = NULL;
    char *sig = NULL;
    if (!m)
        goto fail;
    trust = load(AT_FDCWD, path("/etc/y2linux/update-trust.json"));
    compat = load(AT_FDCWD, path("/etc/y2linux/update-compat.json"));
    if (!trust || !compat)
        goto fail;
    if (num(m, "schema") != 1 || num(trust, "schema") != 1 || num(compat, "schema") != 1) {
        failure("schema incompatible");
        goto fail;
    }
    json_object *keys = field(trust, "keys"), *key = NULL;
    if (!keys || !json_object_is_type(keys, json_type_array) ||
        json_object_array_length(keys) > 16) {
        failure("trust store");
        goto fail;
    }
    for (size_t i = 0; i < json_object_array_length(keys); i++) {
        json_object *item = json_object_array_get_idx(keys, i);
        if (!strcmp(str(item, "id"), str(m, "key_id"))) {
            if (key) {
                failure("ambiguous key");
                goto fail;
            }
            key = item;
        }
    }
    unsigned char pub[32];
    if (!key || strcmp(str(key, "status"), "active") || !hex(str(key, "public_key_hex"), 32, pub)) {
        failure("unknown/revoked signing key");
        goto fail;
    }
    sig = readfile(package, "manifest.sig", 64, &siglen);
    if (!sig || siglen != 64 ||
        crypto_sign_verify_detached((const unsigned char *)sig, (const unsigned char *)raw, length,
                                    pub)) {
        failure("invalid signature");
        goto fail;
    }
    /* Nothing from the manifest is acted on before authentication. */
    const char *exact[] = {"product", "hardware_revision", "kernel", "rootfs_contract", NULL};
    for (unsigned i = 0; exact[i]; i++)
        if (!*str(m, exact[i]) || strcmp(str(m, exact[i]), str(compat, exact[i]))) {
            failure("incompatible %s", exact[i]);
            goto fail;
        }
    if (num(m, "layout_schema") != 1 || num(m, "data_schema") != 1 || num(m, "rescue_api") != 1 ||
        num(m, "reborn_database_schema") != 1 || num(m, "sequence") < 1 ||
        num(m, "minimum_sequence") < 0 || num(m, "sequence") < num(key, "minimum_sequence") ||
        num(m, "sequence") > num(key, "maximum_sequence") || !*str(m, "release_version") ||
        !*str(m, "rootfs_version")) {
        failure("version/schema/key policy");
        goto fail;
    }
    unsigned char commit[20];
    if (!hex(str(m, "y2linux_commit"), 20, commit) || !hex(str(m, "reborn_commit"), 20, commit)) {
        failure("source identity");
        goto fail;
    }
    int64_t installed = num(compat, "sequence");
    bool downgrade = num(m, "sequence") <= installed;
    json_object *water = load(basefd, "accepted.json");
    if (water) {
        if (num(water, "sequence") < 0 || num(water, "current_sequence") < 0) {
            json_object_put(water);
            failure("accepted sequence invalid");
            goto fail;
        }
        installed = num(water, "current_sequence");
        downgrade |= num(m, "sequence") <= num(water, "sequence");
        json_object_put(water);
    } else if (errno != ENOENT) {
        failure("accepted sequence unreadable");
        goto fail;
    }
    if (num(m, "minimum_sequence") > installed) {
        failure("minimum compatible release");
        goto fail;
    }
    if (downgrade && !(yes(m, "allow_downgrade") && yes(key, "allow_downgrade") &&
                       !strcmp(str(key, "purpose"), "development"))) {
        failure("downgrade/replay refused");
        goto fail;
    }
    json_object *p = field(m, "payload");
    unsigned char hash[32];
    if (!p || strcmp(str(p, "target"), "Y2ROOT") || strcmp(str(p, "name"), "rootfs.ext4.gz") ||
        strcmp(str(p, "encoding"), "gzip") || num(p, "bytes") < 1 ||
        num(p, "bytes") > (int64_t)LIMIT || num(p, "raw_bytes") < 8 * 1024 * 1024 ||
        num(p, "raw_bytes") > (int64_t)LIMIT || !hex(str(p, "sha256"), 32, hash) ||
        !hex(str(p, "raw_sha256"), 32, hash)) {
        failure("payload contract");
        goto fail;
    }
    if (payload_check) {
        int fd = openfile(package, "rootfs.ext4.gz", O_RDONLY, 0);
        if (fd < 0)
            goto fail;
        bool ok = stream_image(fd, -1, p, false);
        close(fd);
        if (!ok)
            goto fail;
    }
    unsigned char manifest_hash[32];
    crypto_hash_sha256(manifest_hash, (const unsigned char *)raw, length);
    setstr(m, "_manifest_sha256", hash_string(manifest_hash));
    setnum(m, "_previous_sequence", installed);
    free(sig);
    free(raw);
    json_object_put(trust);
    json_object_put(compat);
    error_text[0] = 0;
    return m;
fail:
    free(sig);
    free(raw);
    if (trust)
        json_object_put(trust);
    if (compat)
        json_object_put(compat);
    if (m)
        json_object_put(m);
    return NULL;
}
static bool readtext(const char *name, char *buf, size_t cap) {
    FILE *f = fopen(path(name), "re");
    if (!f)
        return false;
    bool ok = fgets(buf, (int)cap, f) != NULL && !ferror(f);
    fclose(f);
    if (ok)
        buf[strcspn(buf, "\r\n")] = 0;
    return ok;
}
static bool text_is(const char *name, const char *want) {
    char buf[256];
    return readtext(name, buf, sizeof(buf)) && !strcmp(buf, want);
}
static int root_device(void) {
#ifdef Y2_UPDATE_TEST
    if (testroot)
        return openfile(AT_FDCWD, path("/root.img"), O_RDWR, 0);
#endif
    if (!text_is("/etc/y2linux/rescue-update", "1")) {
        failure("rescue environment required");
        return -1;
    }
    /* Fixed sysfs geometry and controller, never a caller-supplied device path. */
    DIR *dir = opendir("/sys/class/block");
    if (!dir) {
        failure("block inventory");
        return -1;
    }
    struct dirent *e;
    int result = -1;
    while ((e = readdir(dir))) {
        if (e->d_name[0] == '.' || strchr(e->d_name, '/'))
            continue;
        char link[320], actual[PATH_MAX], item[PATH_MAX + 32], parent[PATH_MAX], dev[320];
        snprintf(link, sizeof(link), "/sys/class/block/%s", e->d_name);
        if (!realpath(link, actual) || !strstr(actual, "/11230000.mmc/") ||
            !strstr(actual, "/block/"))
            continue;
        snprintf(item, sizeof(item), "%s/partition", link);
        if (access(item, R_OK))
            continue;
        snprintf(item, sizeof(item), "%s/start", link);
        if (!text_is(item, "166912"))
            continue;
        snprintf(item, sizeof(item), "%s/size", link);
        if (!text_is(item, "1679360"))
            continue;
        strcpy(parent, actual);
        char *slash = strrchr(parent, '/');
        if (!slash)
            continue;
        *slash = 0;
        snprintf(item, sizeof(item), "%s/removable", parent);
        if (!text_is(item, "0"))
            continue;
        snprintf(item, sizeof(item), "%s/device/type", parent);
        if (!text_is(item, "MMC"))
            continue;
        snprintf(item, sizeof(item), "%s/size", parent);
        if (!text_is(item, "15203328") && !text_is(item, "15269888"))
            continue;
        snprintf(dev, sizeof(dev), "/dev/%s", e->d_name);
        struct stat st;
        if (lstat(dev, &st) || !S_ISBLK(st.st_mode))
            continue;
        snprintf(item, sizeof(item), "%u:%u", major(st.st_rdev), minor(st.st_rdev));
        FILE *mounts = fopen("/proc/self/mountinfo", "re");
        char line[8192];
        bool mounted = false;
        if (!mounts) {
            failure("mount inventory");
            goto bad;
        }
        while (fgets(line, sizeof(line), mounts)) {
            char device[64];
            if (sscanf(line, "%*u %*u %63s", device) == 1 && !strcmp(device, item))
                mounted = true;
        }
        bool mount_error = ferror(mounts);
        fclose(mounts);
        if (mounted || mount_error) {
            failure("root is mounted/unknown");
            goto bad;
        }
        if (result >= 0) {
            failure("ambiguous internal root");
            goto bad;
        }
        result = open(dev, O_RDWR | O_CLOEXEC | O_EXCL | O_NOFOLLOW);
        if (result < 0) {
            failure("exclusive root open: %s", strerror(errno));
            goto bad;
        }
        uint64_t device_bytes = 0;
        if (ioctl(result, BLKGETSIZE64, &device_bytes) || device_bytes != 1679360ULL * 512) {
            failure("root device capacity");
            goto bad;
        }
        struct stat opened;
        if (fstat(result, &opened) || opened.st_rdev != st.st_rdev) {
            failure("root identity changed");
            goto bad;
        }
    }
    closedir(dir);
    if (result < 0)
        failure("internal root geometry unavailable");
    return result;
bad:
    if (result >= 0)
        close(result);
    closedir(dir);
    return -1;
}
static bool state(json_object *j, const char *value, const char *why) {
    setstr(j, "state", value);
    setstr(j, "failure", why ? why : "");
    return save("state.json", j);
}
static void boot_id(char *buf, size_t n) {
    if (!readtext("/proc/sys/kernel/random/boot_id", buf, n))
        buf[0] = 0;
}
static bool backup_root(int root, json_object *journal) {
    unsigned char header[4096];
    if (pread(root, header, sizeof(header), 0) != sizeof(header))
        return failure("old root header");
    uint64_t bytes = root_bytes(header);
    if (!bytes)
        return failure("old root identity/extent");
    if (!state(journal, "BackingUp", NULL) || !enough(basefd, CHUNK))
        return false;
    unlinkat(basefd, "previous.ext4.gz.tmp", 0);
    int out = openfile(basefd, "previous.ext4.gz.tmp", O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (out < 0)
        return false;
    z_stream z = {0};
    bool ok = false;
    if (deflateInit2(&z, 6, Z_DEFLATED, 15 + 16, 8, Z_DEFAULT_STRATEGY) != Z_OK) {
        failure("backup gzip init");
        close(out);
        return false;
    }
    unsigned char in[CHUNK], encoded[CHUNK], rawsum[32], gzsum[32];
    crypto_hash_sha256_state rh, ch;
    crypto_hash_sha256_init(&rh);
    crypto_hash_sha256_init(&ch);
    uint64_t left = bytes, gzbytes = 0;
    if (lseek(root, 0, SEEK_SET) < 0)
        goto done;
    while (left) {
        size_t count = left < sizeof(in) ? (size_t)left : sizeof(in);
        ssize_t got = read(root, in, count);
        if (got < 0 && errno == EINTR)
            continue;
        if (got <= 0) {
            failure("backup read");
            goto done;
        }
        left -= (uint64_t)got;
        crypto_hash_sha256_update(&rh, in, (size_t)got);
        z.next_in = in;
        z.avail_in = (uInt)got;
        int rc;
        do {
            z.next_out = encoded;
            z.avail_out = sizeof(encoded);
            rc = deflate(&z, left ? Z_NO_FLUSH : Z_FINISH);
            if (rc != Z_OK && rc != Z_STREAM_END) {
                failure("backup compression");
                goto done;
            }
            count = sizeof(encoded) - z.avail_out;
            if (!enough(basefd, count) || !writeall(out, encoded, count))
                goto done;
            crypto_hash_sha256_update(&ch, encoded, count);
            gzbytes += count;
        } while (z.avail_in || z.avail_out == 0 || (!left && rc != Z_STREAM_END));
    }
    if (fsync(out)) {
        failure("backup fsync");
        goto done;
    }
    crypto_hash_sha256_final(&rh, rawsum);
    crypto_hash_sha256_final(&ch, gzsum);
    json_object *p = json_object_new_object();
    setnum(p, "raw_bytes", (int64_t)bytes);
    setnum(p, "bytes", (int64_t)gzbytes);
    setstr(p, "raw_sha256", hash_string(rawsum));
    setstr(p, "sha256", hash_string(gzsum));
    int verifyfd = openfile(basefd, "previous.ext4.gz.tmp", O_RDONLY, 0);
    ok = verifyfd >= 0 && stream_image(verifyfd, -1, p, false);
    if (verifyfd >= 0)
        close(verifyfd);
    if (ok)
        ok = renameat(basefd, "previous.ext4.gz.tmp", basefd, "previous.ext4.gz") == 0 &&
             fsync(basefd) == 0;
    if (ok) {
        json_object_object_add(journal, "backup", p);
        ok = save("state.json", journal);
    } else
        json_object_put(p);
done:
    deflateEnd(&z);
    close(out);
    if (!ok && !error_text[0])
        failure("backup failed");
    return ok;
}
static bool restore(int root, json_object *j, const char *reason) {
    json_object *p = field(j, "backup");
    if (!p)
        return failure("no verified backup");
    int fd = openfile(basefd, "previous.ext4.gz", O_RDONLY, 0);
    if (fd < 0)
        return false;
    bool ok = stream_image(fd, -1, p, false) && state(j, "RollingBack", reason) &&
              stream_image(fd, root, p, false) && fsync(root) == 0 &&
              file_hash(root, (uint64_t)num(p, "raw_bytes"), str(p, "raw_sha256"));
    close(fd);
    if (!ok) {
        char why[192];
        snprintf(why, sizeof(why), "%s", error_text);
        state(j, "RescueRequired", why);
        return false;
    }
    json_object *water = load(basefd, "accepted.json");
    if (!water) {
        if (errno != ENOENT)
            return failure("accepted sequence unreadable on rollback");
        water = json_object_new_object();
        setnum(water, "sequence", 0);
    }
    setnum(water, "current_sequence", num(j, "previous_sequence"));
    ok = save("accepted.json", water) && state(j, "RolledBack", reason);
    json_object_put(water);
    return ok;
}
static bool external_power(void) {
#ifdef Y2_UPDATE_TEST
    if (testroot)
        return text_is("/external-power", "1");
#endif
    DIR *d = opendir("/sys/class/power_supply");
    if (!d)
        return false;
    struct dirent *e;
    bool online = false;
    char p[PATH_MAX];
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.')
            continue;
        snprintf(p, sizeof(p), "/sys/class/power_supply/%s/type", e->d_name);
        if (!text_is(p, "USB") && !text_is(p, "Mains"))
            continue;
        snprintf(p, sizeof(p), "/sys/class/power_supply/%s/online", e->d_name);
        if (text_is(p, "1"))
            online = true;
    }
    closedir(d);
    return online;
}
static bool rescue(json_object *j) {
    char current[64];
    boot_id(current, sizeof(current));
    if (!*current)
        return failure("boot identity missing");
    const char *s = str(j, "state");
    if (!strcmp(s, "Idle") || !strcmp(s, "Acknowledged") || !strcmp(s, "RolledBack") ||
        !strcmp(s, "Failed"))
        return true;
    if (!strcmp(s, "RescueRequired"))
        return failure("manual rescue required; no automatic retry");
    if (!external_power())
        return failure("external power required for rescue update/restore");
    if (!crypto_ready())
        return false;
    int root = root_device();
    if (root < 0)
        return false;
    bool ok = false;
    if (!strcmp(s, "Installing") || !strcmp(s, "RollingBack") || !strcmp(s, "RollbackPending") ||
        (!strcmp(s, "PendingHealth") && strcmp(str(j, "boot_id"), current))) {
        ok = restore(root, j, "unacknowledged/interrupted update");
        goto done;
    }
    if (!strcmp(s, "PendingHealth")) {
        ok = true;
        goto done;
    }
    if (strcmp(s, "Queued") && strcmp(s, "BackingUp")) {
        failure("unknown update state");
        goto done;
    }
    int pkg = openat(basefd, "pending", O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (pkg < 0) {
        failure("pending package missing");
        goto done;
    }
    json_object *m = verify(pkg, true);
    if (!m || strcmp(str(m, "y2linux_commit"), str(j, "y2linux_commit")) ||
        num(m, "sequence") != num(j, "sequence") ||
        strcmp(str(m, "_manifest_sha256"), str(j, "manifest_sha256"))) {
        char why[192];
        snprintf(why, sizeof(why), "%s", m ? "queued identity changed" : error_text);
        if (m)
            json_object_put(m);
        close(pkg);
        ok = state(j, "Failed", why);
        goto done;
    }
    if (!backup_root(root, j)) {
        char why[192];
        snprintf(why, sizeof(why), "%s", error_text);
        ok = state(j, "Failed", why);
        json_object_put(m);
        close(pkg);
        goto done;
    }
    json_object *p = field(m, "payload");
    int fd = openfile(pkg, "rootfs.ext4.gz", O_RDONLY, 0);
    ok = fd >= 0 && state(j, "Installing", NULL) && stream_image(fd, root, p, true) &&
         fsync(root) == 0;
#ifdef Y2_UPDATE_TEST
    if (ok && getenv("Y2_TEST_FAULT") && !strcmp(getenv("Y2_TEST_FAULT"), "readback")) {
        unsigned char b = 0xff;
        if (pwrite(root, &b, 1, 0) != 1)
            ok = false;
    }
#endif
    if (ok)
        ok = file_hash(root, (uint64_t)num(p, "raw_bytes"), str(p, "raw_sha256"));
    if (fd >= 0)
        close(fd);
    if (ok) {
        setstr(j, "boot_id", current);
        ok = state(j, "PendingHealth", NULL);
    } else {
        char why[192];
        snprintf(why, sizeof(why), "%s", error_text);
        ok = restore(root, j, why);
    }
    json_object_put(m);
    close(pkg);
done:
    close(root);
    return ok;
}
static json_object *initial(void) {
    json_object *j = json_object_new_object();
    setnum(j, "schema", 1);
    setstr(j, "state", "Idle");
    return j;
}
int main(int argc, char **argv) {
#ifdef Y2_UPDATE_TEST
    testroot = getenv("Y2_TEST_ROOT");
    if (!testroot || testroot[0] != '/') {
        fputs("test root required\n", stderr);
        return 2;
    }
#endif
    if (argc < 2 || argc > 3)
        return 2;
    bool isrescue = !strcmp(argv[1], "rescue") || !strcmp(argv[1], "rescue-restore") ||
                    !strcmp(argv[1], "rescue-needed");
    const char *base = path(isrescue ? "/newdata/updates" : "/data/updates");
    basefd = open(base, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (basefd < 0) {
        failure("update state directory unavailable");
        goto output;
    }
    struct stat bst;
    if (fstat(basefd, &bst) || (bst.st_mode & 0077) || bst.st_uid != geteuid()) {
        failure("update state permissions");
        goto output;
    }
    if (strcmp(argv[1], "rescue-needed")) {
        int lock = openfile(basefd, ".lock", O_RDWR | O_CREAT, 0600);
        if (lock < 0 || flock(lock, LOCK_EX | LOCK_NB)) {
            failure("update busy");
            goto output;
        }
    }
    bool ok = false;
    json_object *j = load(basefd, "state.json");
    if (!j) {
        if (errno == ENOENT) {
            j = initial();
            error_text[0] = 0;
        } else {
            failure("update journal unreadable");
            goto output;
        }
    }
    if (num(j, "schema") != 1 || !*str(j, "state")) {
        json_object_put(j);
        failure("update journal schema");
        goto output;
    }
    if (!strcmp(argv[1], "rescue-needed") && argc == 2) {
        const char *s = str(j, "state");
        bool idle = !strcmp(s, "Idle") || !strcmp(s, "Acknowledged") || !strcmp(s, "RolledBack") ||
                    !strcmp(s, "Failed");
        bool install = !strcmp(s, "Queued") || !strcmp(s, "BackingUp");
        puts(json_object_to_json_string_ext(j, JSON_C_TO_STRING_PLAIN));
        json_object_put(j);
        close(basefd);
        return idle ? 10 : install ? 0 : 11;
    }
    if ((!strcmp(argv[1], "check") || !strcmp(argv[1], "check-manifest")) && argc == 3) {
        int pkg = open(path(argv[2]), O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
        json_object *m = pkg >= 0 ? verify(pkg, strcmp(argv[1], "check-manifest") != 0) : NULL;
        if (pkg >= 0)
            close(pkg);
        if (m) {
            puts(json_object_to_json_string_ext(m, JSON_C_TO_STRING_PLAIN));
            json_object_put(m);
            json_object_put(j);
            return 0;
        }
        if (!error_text[0])
            failure("package unavailable");
    } else if (!strcmp(argv[1], "queue") && argc == 2) {
        const char *s = str(j, "state");
        if (strcmp(s, "Idle") && strcmp(s, "Acknowledged") && strcmp(s, "RolledBack") &&
            strcmp(s, "Failed"))
            failure("update already pending");
        else {
            int pkg = openat(basefd, "pending", O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
            json_object *m = pkg >= 0 ? verify(pkg, true) : NULL;
            if (pkg >= 0)
                close(pkg);
            if (m) {
                json_object_put(j);
                j = initial();
                setnum(j, "sequence", num(m, "sequence"));
                setnum(j, "previous_sequence", num(m, "_previous_sequence"));
                setstr(j, "manifest_sha256", str(m, "_manifest_sha256"));
                setstr(j, "y2linux_commit", str(m, "y2linux_commit"));
                setstr(j, "release_version", str(m, "release_version"));
                json_object_object_add(j, "rollback_allowed",
                                       json_object_new_boolean(yes(m, "rollback_allowed")));
                ok = state(j, "Queued", NULL);
                json_object_put(m);
            }
        }
    } else if (isrescue && argc == 2) {
        if (!strcmp(argv[1], "rescue-restore")) {
            if (!field(j, "backup")) {
                failure("verified backup unavailable");
            } else if (state(j, "RollbackPending", "owner rescue restore"))
                ok = rescue(j);
        } else
            ok = rescue(j);
    } else if (!strcmp(argv[1], "status") && argc == 2)
        ok = true;
    else if (!strcmp(argv[1], "ack") && argc == 2) {
        char current[64];
        boot_id(current, sizeof(current));
        json_object *versions = load(AT_FDCWD, path("/etc/y2linux/versions.json"));
        json_object *ready = load(AT_FDCWD, path("/run/y2/application-ready.json"));
        if (strcmp(str(j, "state"), "PendingHealth") || strcmp(str(j, "boot_id"), current) ||
            !versions || strcmp(str(versions, "build_git_commit"), str(j, "y2linux_commit")) ||
            !ready || strcmp(str(ready, "boot_id"), current))
            failure("boot-health acknowledgement prerequisites");
        else {
            json_object *water = load(basefd, "accepted.json");
            if (!water && errno != ENOENT) {
                failure("accepted sequence unreadable");
                goto ack_done;
            }
            if (!water)
                water = json_object_new_object();
            int64_t highest = num(water, "sequence");
            setnum(water, "sequence", highest > num(j, "sequence") ? highest : num(j, "sequence"));
            setnum(water, "current_sequence", num(j, "sequence"));
            ok = save("accepted.json", water) && state(j, "Acknowledged", NULL);
            json_object_put(water);
        }
    ack_done:
        if (versions)
            json_object_put(versions);
        if (ready)
            json_object_put(ready);
    } else if (!strcmp(argv[1], "cancel") && argc == 2) {
        if (strcmp(str(j, "state"), "Queued"))
            failure("only queued updates can be cancelled");
        else
            ok = state(j, "Failed", "owner cancelled before rescue installation");
    } else if (!strcmp(argv[1], "rollback") && argc == 2) {
        if (!field(j, "backup") ||
            !(yes(j, "rollback_allowed") || !strcmp(str(j, "state"), "PendingHealth")))
            failure("rollback policy/backup unavailable");
        else
            ok = state(j, "RollbackPending", "owner/health requested restore");
    } else
        failure("unknown command");
    if (ok) {
        error_text[0] = 0;
        puts(json_object_to_json_string_ext(j, JSON_C_TO_STRING_PLAIN));
        json_object_put(j);
        close(basefd);
        return 0;
    }
    json_object_put(j);
output: {
    json_object *e = json_object_new_object();
    setstr(e, "state", "Failed");
    setstr(e, "failure", *error_text ? error_text : "operation failed");
    puts(json_object_to_json_string_ext(e, JSON_C_TO_STRING_PLAIN));
    json_object_put(e);
}
    if (basefd >= 0)
        close(basefd);
    return 1;
}
