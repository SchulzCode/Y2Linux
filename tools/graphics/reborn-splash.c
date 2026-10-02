/* SPDX-License-Identifier: MIT
 * Early normal-boot KMS owner. One CPU-mapped dumb buffer; no GPU/context,
 * framebuffer-console rendering, shell backend, radio or storage operations.
 * /run and /dev directory fds survive initramfs switch_root/mount moves.
 *
 * It shows the Reborn wordmark, one thin bar and one status line. The bar and
 * status follow real startup milestones: producers (the initramfs, Reborn)
 * write a milestone name to /run/reborn-splash/phase; names map to coarse bar
 * positions through the generated table in reborn-splash-mark.h. The bar only
 * moves forward, never advances on a timer and redraws only while it moves.
 * Reborn's READY completes the bar and hands the display over at once. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sys/inotify.h>
#include <linux/kd.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#include <drm_fourcc.h>
#include <xf86drm.h>
#include <xf86drmMode.h>
#include "reborn-splash-mark.h"

#define SOCKET_PATH "/run/reborn-splash/control.sock"
#define TIMEOUT_MS 60000
#define HANDOFF_MS 3000
#define BG RB_BG
/* The bar glides at least 1/4 px per tick and a quarter-eighth of its distance. */
#define FILL_UNIT 256u
#define TICK_MS 33
static int dirfd = -1, devfd = -1, logfd = -1, journal = -1;
static unsigned log_bytes, sequence;
static volatile sig_atomic_t stopped;
static uint64_t now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static void event(const char *name, int error) {
    char b[256];
    int n = snprintf(b, sizeof(b), "{\"subsystem\":\"startup\",\"event\":\"splash_%s\",\"mono_ms\":%llu,\"sequence\":%u,\"errno\":%d}\n",
                     name, (unsigned long long)now_ms(), ++sequence, error);
    if (logfd >= 0 && write(logfd, b, (size_t)n) != n) { close(logfd); logfd=-1; }
    if (journal >= 0 && log_bytes + (unsigned)n <= 8192) {
        if (write(journal, b, (size_t)n) != n) { close(journal); journal=-1; }
        log_bytes += (unsigned)n;
    }
}
static void phase_event(const char *name, const char *phase) {
    char b[256];
    int n = snprintf(b, sizeof(b), "{\"subsystem\":\"startup\",\"event\":\"splash_%s\",\"phase\":\"%s\",\"mono_ms\":%llu,\"sequence\":%u}\n",
                     name, phase, (unsigned long long)now_ms(), ++sequence);
    if (logfd >= 0 && write(logfd, b, (size_t)n) != n) { close(logfd); logfd=-1; }
    if (journal >= 0 && log_bytes + (unsigned)n <= 8192) {
        if (write(journal, b, (size_t)n) != n) { close(journal); journal=-1; }
        log_bytes += (unsigned)n;
    }
}
static void state(const char *name) {
    int fd = openat(dirfd, "state.json", O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC|O_NOFOLLOW, 0600);
    if (fd >= 0) {
        dprintf(fd, "{\"state\":\"%s\",\"pid\":%ld,\"mono_ms\":%llu}\n", name,
                (long)getpid(), (unsigned long long)now_ms()); close(fd);
    }
}
static void signal_stop(int sig) { (void)sig; stopped = 1; }
typedef struct {
    int fd;
    uint32_t connector, crtc, fb, handle, pitch;
    unsigned width, height;
    size_t size;
    uint8_t *pixels;
    drmModeModeInfo mode;
} Display;
static void display_close(Display *d) {
    if (d->pixels) munmap(d->pixels, d->size);
    if (d->fb) drmModeRmFB(d->fd, d->fb);
    if (d->handle) {
        struct drm_mode_destroy_dumb destroy = {.handle=d->handle};
        drmIoctl(d->fd, DRM_IOCTL_MODE_DESTROY_DUMB, &destroy);
    }
    if (d->fd >= 0) close(d->fd);
    *d = (Display){.fd=-1};
}
static int display_open(Display *d) {
    for (unsigned i=0; i<16; i++) {
        char path[40]; snprintf(path,sizeof(path),"dri/card%u",i);
        int fd = openat(devfd, path, O_RDWR|O_CLOEXEC);
        if (fd < 0) continue;
        drmVersionPtr v=drmGetVersion(fd);
        bool match=v && v->name && !strcmp(v->name,"mediatek");
        drmFreeVersion(v);
        if (match) { d->fd=fd; break; } close(fd);
    }
    if (d->fd < 0 || drmSetMaster(d->fd)) goto fail;
    drmModeRes *r=drmModeGetResources(d->fd);
    if (!r) goto fail;
    for (int i=0; i<r->count_connectors && !d->connector; i++) {
        drmModeConnector *c=drmModeGetConnector(d->fd,r->connectors[i]);
        if (c && c->connection==DRM_MODE_CONNECTED && c->count_modes) {
            d->connector=c->connector_id; d->mode=c->modes[0];
            for (int k=0; k<c->count_modes; k++)
                if (c->modes[k].type & DRM_MODE_TYPE_PREFERRED) { d->mode=c->modes[k]; break; }
            for (int k=0; k<c->count_encoders && !d->crtc; k++) {
                drmModeEncoder *e=drmModeGetEncoder(d->fd,c->encoders[k]);
                if (e) {
                    d->crtc=e->crtc_id;
                    for (int j=0; !d->crtc && j<r->count_crtcs; j++)
                        if (e->possible_crtcs & (1U<<j)) d->crtc=r->crtcs[j];
                    drmModeFreeEncoder(e);
                }
            }
        }
        drmModeFreeConnector(c);
    }
    drmModeFreeResources(r);
    d->width=d->mode.hdisplay; d->height=d->mode.vdisplay;
    if (!d->crtc || d->width<320 || d->width>1024 || d->height<240 || d->height>1024) goto fail;
    struct drm_mode_create_dumb create={.width=d->width,.height=d->height,.bpp=32};
    if (drmIoctl(d->fd,DRM_IOCTL_MODE_CREATE_DUMB,&create)) goto fail;
    d->handle=create.handle; d->pitch=create.pitch; d->size=create.size;
    if (d->pitch < d->width*4 || d->size < (uint64_t)d->pitch*d->height || d->size>8*1024*1024) goto fail;
    uint32_t handles[4]={d->handle}, pitches[4]={d->pitch}, offsets[4]={0};
    if (drmModeAddFB2(d->fd,d->width,d->height,DRM_FORMAT_XRGB8888,handles,pitches,offsets,&d->fb,0)) goto fail;
    struct drm_mode_map_dumb map={.handle=d->handle};
    if (drmIoctl(d->fd,DRM_IOCTL_MODE_MAP_DUMB,&map)) goto fail;
    d->pixels=mmap(NULL,d->size,PROT_READ|PROT_WRITE,MAP_SHARED,d->fd,map.offset);
    if (d->pixels==MAP_FAILED) { d->pixels=NULL; goto fail; }
    return 0;
fail:
    display_close(d); return -1;
}
static void rect(Display *d, unsigned x, unsigned y, unsigned w, unsigned h, uint32_t rgb) {
    if (!d->pixels || x+w>d->width || y+h>d->height) return;
    for (unsigned row=y; row<y+h; row++) {
        uint32_t *p=(uint32_t *)(d->pixels+(size_t)row*d->pitch)+x;
        for (unsigned col=0; col<w; col++) p[col]=rgb;
    }
}
/* An image decoded from Reborn's own rendered screens (RGB888 runs), so the
 * hand-off to Reborn's first frame is pixel-identical. Coordinates are those of
 * the 480x360 screen, centered on panels that are not 480x360. */
static void blit(Display *d, const rb_image *image) {
    unsigned ox = (d->width - 480) / 2, oy = (d->height - 360) / 2;
    const unsigned char *rle = rb_art_rle + image->offset;
    size_t offset = 0;
    unsigned remaining = 0;
    uint32_t rgb = BG;
    for (unsigned y = 0; y < image->h; y++) {
        uint32_t *row = (uint32_t *)(d->pixels + (size_t)(oy + image->y + y) * d->pitch);
        for (unsigned x = 0; x < image->w; x++) {
            if (!remaining) {
                if (offset + 4 > image->length) return;
                remaining = rle[offset];
                rgb = (uint32_t)rle[offset + 1] << 16 | (uint32_t)rle[offset + 2] << 8 | rle[offset + 3];
                offset += 4;
            }
            row[ox + image->x + x] = rgb;
            remaining--;
        }
    }
}
/* Startup progress: the furthest real milestone reached and the animated bar
 * position, in 1/FILL_UNIT pixels, easing toward that milestone's position. */
typedef struct { unsigned phase, shown, target; bool failed; } Progress;
static unsigned fill_units(unsigned phase) {
    return (unsigned)((uint64_t)rb_phases[phase].fill * RB_BAR_W * FILL_UNIT / 1000);
}
static int phase_find(const char *name) {
    for (unsigned i = 0; i < RB_PHASE_COUNT; i++) if (!strcmp(rb_phases[i].token, name)) return (int)i;
    return -1;
}
static bool phase_fails(const char *name) {
    for (unsigned i = 0; i < RB_FAILURE_TOKEN_COUNT; i++) if (!strcmp(rb_failure_tokens[i], name)) return true;
    return false;
}
/* Milestone names are short lowercase identifiers; anything else is ignored. */
static bool phase_clean(char *name) {
    size_t n = strlen(name);
    while (n && (name[n - 1] == '\n' || name[n - 1] == '\r' || name[n - 1] == ' ')) name[--n] = 0;
    if (!n || n > 39) return false;
    for (size_t i = 0; i < n; i++)
        if (!((name[i] >= 'a' && name[i] <= 'z') || (name[i] >= '0' && name[i] <= '9') || name[i] == '_')) return false;
    return true;
}
/* Apply one reported milestone. Unknown names, repeats and anything earlier
 * than the furthest milestone are ignored, so the bar can never regress and a
 * missing milestone just leaves it where it is until a later one arrives. */
static bool progress_report(Progress *p, const char *name) {
    if (phase_fails(name)) {
        if (p->failed) return false;
        p->failed = true;
        return true;
    }
    int i = phase_find(name);
    if (i < 0 || (unsigned)i <= p->phase) return false;
    p->phase = (unsigned)i; p->target = fill_units(p->phase);
    return true;
}
/* One animation tick toward the target; false once there. */
static bool progress_step(Progress *p) {
    if (p->shown >= p->target) return false;
    unsigned delta = p->target - p->shown, step = delta / 8;
    if (step < FILL_UNIT / 4) step = FILL_UNIT / 4;
    p->shown = delta <= step ? p->target : p->shown + step;
    return true;
}
/* Reborn is ready: complete the bar at once; a late start is no longer a failure. */
static void progress_finish(Progress *p) {
    p->phase = RB_PHASE_COUNT - 1; p->shown = p->target = fill_units(p->phase);
    p->failed = false;
}
static uint32_t mix(uint32_t from, uint32_t to, unsigned level /* 0..255 */) {
    uint32_t out = 0;
    for (unsigned shift = 0; shift <= 16; shift += 8) {
        unsigned a = (from >> shift) & 0xff, b = (to >> shift) & 0xff;
        out |= (uint32_t)((a * (255 - level) + b * level + 127) / 255) << shift;
    }
    return out;
}
/* The thin track with its fill from the left; the leading pixel is blended so
 * the fill advances smoothly in sub-pixel steps. */
static void bar(Display *d, unsigned shown) {
    unsigned ox = (d->width - 480) / 2, oy = (d->height - 360) / 2;
    unsigned whole = shown / FILL_UNIT, fraction = shown % FILL_UNIT;
    for (unsigned row = 0; row < RB_BAR_H; row++) {
        uint32_t *p = (uint32_t *)(d->pixels + (size_t)(oy + RB_BAR_Y + row) * d->pitch) + ox + RB_BAR_X;
        for (unsigned x = 0; x < RB_BAR_W; x++)
            p[x] = x < whole ? RB_BAR_FILL : x == whole && fraction ? mix(RB_BAR_TRACK, RB_BAR_FILL, fraction) : RB_BAR_TRACK;
    }
}
static void status(Display *d, unsigned phase) {
    unsigned ox = (d->width - 480) / 2, oy = (d->height - 360) / 2;
    rect(d, ox + RB_LABEL_X, oy + RB_LABEL_Y, RB_LABEL_W, RB_LABEL_H, BG);
    blit(d, &rb_labels[rb_phases[phase].label]);
}
static bool label_changed(unsigned from, unsigned to) {
    return rb_phases[from].label != rb_phases[to].label;
}
static void draw(Display *d, const Progress *p, bool failure) {
    if (!d->pixels || d->width < 480 || d->height < 360) return;
    rect(d, 0, 0, d->width, d->height, BG);
    blit(d, &rb_mark);
    if (failure) {
        for (unsigned i = 0; i < RB_FAILURE_LABEL_COUNT; i++) blit(d, &rb_labels[rb_failure_labels[i]]);
    } else {
        bar(d, p->shown);
        status(d, p->phase);
    }
}
static int show(Display *d) {
    return drmModeSetCrtc(d->fd,d->crtc,d->fb,0,0,&d->connector,1,&d->mode);
}
#ifdef RB_SPLASH_TEST
/* Protocol test builds never open devices; not included in the shipped binary. */
static int claim(Display *d) { (void)d; event("test_claim",0); return 0; }
static int release(Display *d) { (void)d; event("test_release",0); return 0; }
#else
static int claim(Display *d) { return d->fd < 0 ? 0 : drmSetMaster(d->fd); }
static int release(Display *d) { return d->fd < 0 ? 0 : drmDropMaster(d->fd); }
#endif
/* The latest milestone a producer reported, or false if there is none. */
static bool read_phase(char *name, size_t size) {
    int fd = openat(dirfd, "phase", O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
    if (fd < 0) return false;
    ssize_t n = read(fd, name, size - 1);
    close(fd);
    if (n <= 0) return false;
    name[n] = 0;
    return phase_clean(name);
}
static void drain_notifications(int notify) {
    char buffer[512] __attribute__((aligned(__alignof__(struct inotify_event))));
    while (read(notify, buffer, sizeof(buffer)) > 0) {}
}
static int serve(int listener, int notify, unsigned timeout, bool test) {
    Display d={.fd=-1};
    Progress progress={0};
    int client=-1;
    bool released=false, failure=false, painted=false;
    size_t used=0; char request[32], reported[48], ignored[48]={0};
    /* The failure timeout is a stall timeout: it counts from the last accepted
     * milestone, so a slow but progressing startup is never called a failure. */
    uint64_t start=now_ms(), deadline=0, retry=0, tick=0, progressed=start;
    state("loading"); event("started",0);
    bool check_phase=true;
    while (!stopped) {
        uint64_t now=now_ms();
        if (!released && !test && d.fd<0 && now>=retry && now-start<20000) {
            retry=now+100;
            if (!display_open(&d)) {
                draw(&d,&progress,failure);
                if (show(&d)) display_close(&d);
                else { painted=true; event("visible",0); }
            }
        }
        if (check_phase) {
            check_phase=false;
            if (read_phase(reported,sizeof(reported))) {
                unsigned before=progress.phase; bool was_failed=progress.failed;
                if (progress_report(&progress,reported)) {
                    phase_event("phase",reported);
                    if (progress.failed && !was_failed) {
                        failure=true; state("failed");
                        if (!released && painted) { draw(&d,&progress,true); }
                    } else if (progress.phase!=before) {
                        tick=now; progressed=now;
                        if (!released && painted && !failure && label_changed(before,progress.phase))
                            status(&d,progress.phase);
                    }
                } else if (strcmp(reported,ignored) && phase_find(reported)!=(int)progress.phase) {
                    /* Unknown, or earlier than the furthest milestone: ignored once. */
                    snprintf(ignored,sizeof(ignored),"%s",reported);
                    phase_event("phase_ignored",reported);
                }
            }
        }
        if (!failure && now-progressed>=timeout) {
            failure=true; state("timeout"); event("timeout",0);
            if (!released) draw(&d,&progress,true);
        }
        /* The bar only redraws while it is still moving toward a milestone. */
        bool animating=!released && !failure && painted && progress.shown<progress.target;
        if (animating && now>=tick) {
            progress_step(&progress); bar(&d,progress.shown); tick=now+TICK_MS;
            animating=progress.shown<progress.target;
        }
        int64_t wait=-1;
#define SOONER(at) do { int64_t m_=(int64_t)(at)-(int64_t)now_ms(); if (m_<0) m_=0; if (wait<0||m_<wait) wait=m_; } while (0)
        if (animating) SOONER(tick);
        if (!released && !test && d.fd<0 && now-start<20000) SOONER(retry);
        if (!failure) SOONER(progressed+timeout);
        if (client>=0) SOONER(deadline);
        if (notify<0) { if (wait<0||wait>100) wait=100; check_phase=true; }
#undef SOONER
        struct pollfd p[3]={{.fd=listener,.events=POLLIN},{.fd=client,.events=POLLIN},{.fd=notify,.events=POLLIN}};
        int result=poll(p,3,wait>INT32_MAX?INT32_MAX:(int)wait);
        if (result<0 && errno!=EINTR) break;
        if (p[2].revents&POLLIN) { drain_notifications(notify); check_phase=true; }
        if (p[0].revents&POLLIN) {
            int fd=accept4(listener,NULL,NULL,SOCK_NONBLOCK|SOCK_CLOEXEC);
            struct ucred cred; socklen_t n=sizeof(cred);
            if (fd>=0) {
                if (client>=0 || getsockopt(fd,SOL_SOCKET,SO_PEERCRED,&cred,&n) || cred.uid!=geteuid()) close(fd);
                else { client=fd; used=0; deadline=now_ms()+HANDOFF_MS; }
            }
        }
        if (client>=0 && (p[1].revents&(POLLIN|POLLHUP|POLLERR))) {
            ssize_t n=recv(client,request+used,sizeof(request)-1-used,0);
            if (n>0) {
                used+=(size_t)n; request[used]=0;
                if (strchr(request,'\n')) {
                    bool ready=!released && !strcmp(request,"READY 1\n");
                    /* Complete the bar and status, then release: the first
                     * Reborn frame is exactly this screen. No further wait. */
                    if (ready) {
                        unsigned before=progress.phase; bool failed=failure;
                        progress_finish(&progress); failure=false; check_phase=false;
                        if (painted && failed) draw(&d,&progress,false);
                        else if (painted) {
                            bar(&d,progress.shown);
                            if (label_changed(before,progress.phase)) status(&d,progress.phase);
                        }
                    }
                    if (ready && !release(&d)) {
                        released=true; state("handoff"); event("released",0);
                        if (send(client,"RELEASED 1\n",11,MSG_NOSIGNAL)!=11) deadline=0;
                        else deadline=now_ms()+HANDOFF_MS;
                        used=0;
                    } else if (released && !strcmp(request,"PRESENTED 1\n")) {
                        state("presented"); event("presented",0);
                        int done=openat(dirfd,"done",O_WRONLY|O_CREAT|O_CLOEXEC|O_NOFOLLOW,0600);
                        if (done>=0) close(done);
                        /* Reborn has an open DRM fd and its own scanout now. Neither
                         * old CRTC restoration nor last-close fbcon restoration occurs. */
                        close(client); display_close(&d); return 0;
                    } else deadline=0;
                } else if (used==sizeof(request)-1) deadline=0;
            } else if (!n || (errno!=EAGAIN && errno!=EINTR)) deadline=0;
        }
        if (client>=0 && now_ms()>=deadline) {
            close(client); client=-1; used=0;
            if (released) event("handoff_aborted",0);
        }
        if (released && client<0 && !claim(&d)) {
            released=false; failure=true; state("handoff_failed");
            draw(&d,&progress,true);
            if (!test && painted) (void)show(&d);
        }
    }
    if (client>=0) close(client);
    /* Never change KD_GRAPHICS to KD_TEXT, even on a service failure. */
    display_close(&d); state("stopped"); event("stopped",0); return 1;
}
static bool normal_metadata(const char *s) {
    int valid,mode,reason,offline;
    return sscanf(s,"valid=%d boot_mode=%d boot_reason=%d offline=%d",&valid,&mode,&reason,&offline)==4 && valid==1 && offline==0;
}
static int hide_console(void) {
    int tty=open("/dev/tty0",O_RDWR|O_CLOEXEC);
    if (tty<0 || ioctl(tty,KDSETMODE,KD_GRAPHICS)) { event("console_suppression_failed",errno); if(tty>=0)close(tty); return 1; }
    close(tty); event("console_hidden",0); return 0;
}
static int number(const char *path) {
    char b[32]={0}; int fd=open(path,O_RDONLY|O_CLOEXEC);
    if (fd<0) return -1;
    ssize_t n=read(fd,b,sizeof(b)-1); close(fd);
    if (n<=0) return -1;
    char *end; long v=strtol(b,&end,10);
    return end!=b && (*end==0 || *end=='\n') && v>=0 && v<10000000 ? (int)v : -1;
}
int main(int argc, char **argv) {
    bool test=false; unsigned timeout=TIMEOUT_MS;
    const char *directory="/run/reborn-splash", *socket_path=SOCKET_PATH;
    if (argc==2 && !strcmp(argv[1],"--quiet-if-normal")) {
        char b[160]={0}; int f=open("/sys/firmware/y2_boot/metadata",O_RDONLY|O_CLOEXEC);
        if (f<0) return 0;
        ssize_t n=read(f,b,sizeof(b)-1); close(f);
        if (n<=0 || !normal_metadata(b)) return 0;
        /* Mirror the unchanged charger's normal-entry voltage gate. Leave
         * charger-triggered and low-battery charging display policy untouched. */
        int voltage=-1, present=-1;
        for (unsigned retry=0; retry<50; retry++) {
            voltage=number("/sys/class/power_supply/BAT0/voltage_now");
            present=number("/sys/class/power_supply/BAT0/present");
            if (voltage>0 && present==1) break;
            poll(NULL,0,100);
        }
        if (voltage<3400000 || present!=1) return 0;
        /* No DRM owner/process is started until the charger/voltage gate permits
         * normal boot. Low-battery normal entries still use offline charging. */
        logfd=open("/dev/kmsg",O_WRONLY|O_CLOEXEC);
        return hide_console();
    } else if (argc==2 && !strcmp(argv[1],"--start")) {
        /* Called only after the unchanged offline charger allows normal boot. */
#ifdef RB_SPLASH_TEST
    } else if (argc==4 && !strcmp(argv[1],"--test")) {
        test=true; directory=argv[2]; socket_path=argv[3]; timeout=250;
#endif
    } else return 2;
    umask(077);
    if (mkdir(directory,0700) && errno!=EEXIST) return 1;
    dirfd=open(directory,O_DIRECTORY|O_RDONLY|O_NOFOLLOW|O_CLOEXEC);
    if (dirfd<0) return 1;
    int lock=openat(dirfd,"lock",O_RDWR|O_CREAT|O_CLOEXEC|O_NOFOLLOW,0600);
    if (lock<0) return 1;
    if (flock(lock,LOCK_EX|LOCK_NB)) return errno==EWOULDBLOCK ? 0 : 1;
    if (!faccessat(dirfd,"done",F_OK,AT_SYMLINK_NOFOLLOW)) return 0;
    if (!test) logfd=open("/dev/kmsg",O_WRONLY|O_CLOEXEC);
    journal=openat(dirfd,"events.jsonl",O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC|O_NOFOLLOW,0600);
    if (!test) {
        if (hide_console()) return 1;
        devfd=open("/dev",O_RDONLY|O_DIRECTORY|O_CLOEXEC);
    }
    int listener=socket(AF_UNIX,SOCK_STREAM|SOCK_NONBLOCK|SOCK_CLOEXEC,0);
    struct sockaddr_un address={.sun_family=AF_UNIX};
    if (strlen(socket_path)>=sizeof(address.sun_path)) return 1;
    strcpy(address.sun_path,socket_path);
    unlinkat(dirfd,"control.sock",0);
    if (listener<0 || bind(listener,(struct sockaddr *)&address,sizeof(address)) ||
        fchmodat(dirfd,"control.sock",0600,0) || listen(listener,2)) return 1;
    if (!test) {
        pid_t pid=fork(); if (pid<0) return 1; if (pid>0) return 0;
        setsid();
        int null=open("/dev/null",O_RDWR|O_CLOEXEC);
        if (null>=0) { for (int i=0;i<3;i++) dup2(null,i); if (null>2) close(null); }
    }
    signal(SIGTERM,signal_stop); signal(SIGINT,signal_stop);
    /* Producers write the phase file; watching the directory costs nothing
     * while startup is quiet. Without inotify the loop falls back to a poll. */
    int notify=inotify_init1(IN_NONBLOCK|IN_CLOEXEC);
    if (notify>=0 && inotify_add_watch(notify,directory,IN_CLOSE_WRITE|IN_MOVED_TO)<0) { close(notify); notify=-1; }
    int result=serve(listener,notify,timeout,test);
    unlinkat(dirfd,"control.sock",0); close(listener); return result;
}
