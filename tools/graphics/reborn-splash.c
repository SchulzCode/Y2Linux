/* SPDX-License-Identifier: MIT
 * Early normal-boot KMS owner. One CPU-mapped dumb buffer; no GPU/context,
 * framebuffer-console rendering, shell backend, radio or storage operations.
 * /run and /dev directory fds survive initramfs switch_root/mount moves. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
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
#include "reborn-splash-font.h"

#define SOCKET_PATH "/run/reborn-splash/control.sock"
#define TIMEOUT_MS 60000
#define HANDOFF_MS 3000
#define BG 0x090c12
#define ACCENT 0x83e3b8
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
    if (logfd >= 0) (void)write(logfd, b, (size_t)n);
    if (journal >= 0 && log_bytes + (unsigned)n <= 8192) {
        (void)write(journal, b, (size_t)n); log_bytes += (unsigned)n;
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
static void text(Display *d, const char *s, unsigned y, unsigned scale, uint32_t color) {
    size_t n=strlen(s); if (!n || n*8*scale>d->width) return;
    unsigned x=(d->width-(unsigned)n*8*scale)/2;
    for (size_t c=0; c<n; c++) for (unsigned row=0; row<8; row++)
        for (unsigned col=0; col<8; col++) if (rb_font[(unsigned char)s[c]&127][row] & (1U<<col))
            rect(d,x+(unsigned)c*8*scale+col*scale,y+row*scale,scale,scale,color);
}
static void draw(Display *d, bool failure, uint64_t elapsed, bool full) {
    unsigned mid=d->height/2;
    if (full) {
        rect(d,0,0,d->width,d->height,BG);
        text(d,"REBORN",mid-68,4,ACCENT);
        text(d,failure ? "Startup needs attention" : "Starting your music",mid,2,0xd9e4ee);
        if (failure) text(d,"Diagnostics available over USB",mid+58,1,0xaac1d5);
    }
    if (!failure) {
        unsigned phase=(unsigned)(elapsed%2400)*2;
        unsigned position=(phase>2400 ? 4800-phase : phase)*160/2400;
        rect(d,d->width/2-96,mid+48,192,4,0x26394a);
        rect(d,d->width/2-96+position,mid+48,32,4,ACCENT);
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
static int serve(int listener, unsigned timeout, bool test) {
    Display d={.fd=-1};
    int client=-1;
    bool released=false, failure=false, painted=false;
    size_t used=0; char request[32];
    uint64_t start=now_ms(), deadline=0, retry=0;
    state("loading"); event("started",0);
    while (!stopped) {
        uint64_t now=now_ms();
        if (!released && !test && d.fd<0 && now>=retry && now-start<20000) {
            retry=now+100;
            if (!display_open(&d)) {
                draw(&d,failure,now-start,true);
                if (show(&d)) display_close(&d);
                else { painted=true; event("visible",0); }
            }
        }
        if (!failure && now-start>=timeout) {
            failure=true; state("timeout"); event("timeout",0);
            if (!released) draw(&d,true,now-start,true);
        }
        if (!released && !failure && painted) draw(&d,false,now-start,false);
        struct pollfd p[2]={{.fd=listener,.events=POLLIN},{.fd=client,.events=POLLIN}};
        int wait=(failure || released) ? 250 : 50;
        int result=poll(p,2,wait);
        if (result<0 && errno!=EINTR) break;
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
                    if (!released && !strcmp(request,"READY 1\n") && !release(&d)) {
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
            draw(&d,true,now_ms()-start,true);
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
    int result=serve(listener,timeout,test);
    unlinkat(dirfd,"control.sock",0); close(listener); return result;
}
