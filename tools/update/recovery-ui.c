/* SPDX-License-Identifier: GPL-2.0-only
 * Y2 recovery screen. One CPU-mapped dumb buffer, no GPU, no input. It shows
 * what the recovery installer is doing:
 *   /run/y2/recovery-screen    title line, then up to five body lines
 *   /run/y2/recovery-progress  "<percent> <step>" written by y2-update-core
 * and redraws only when either file changes. The installer never depends on
 * this process: a missing panel just means an update without a picture.
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <drm_fourcc.h>
#include <xf86drm.h>
#include <xf86drmMode.h>
#include "font-ter16x32.h"

/* Reborn's boot palette (tools/graphics/reborn-splash-mark.h). */
#define BG 0x06070au
#define TEXT 0xf6f6f8u
#define DIM 0x9a9ba3u
#define TRACK 0x2d2e30u
#define FILL 0xf6f6f8u
#define LINES 6
#define COLUMNS 28

/* Two dumb buffers: draw() always writes the one that is not scanned out
 * (pixels/fb), present() flips to it at vblank. Drawing into the visible
 * buffer showed half-drawn frames (owner, 38: the screen flickered). */
typedef struct {
    int fd;
    uint32_t connector, crtc, fb, handle, pitch;
    unsigned width, height;
    size_t size;
    uint8_t *pixels;
    drmModeModeInfo mode;
    uint32_t fbs[2];
    uint8_t *maps[2];
    unsigned back;
} Display;

static int display_open(Display *d) {
    *d = (Display){.fd = -1};
    for (unsigned i = 0; i < 16 && d->fd < 0; i++) {
        char path[40];
        snprintf(path, sizeof(path), "/dev/dri/card%u", i);
        int fd = open(path, O_RDWR | O_CLOEXEC);
        if (fd < 0)
            continue;
        drmVersionPtr v = drmGetVersion(fd);
        bool match = v && v->name && !strcmp(v->name, "mediatek");
        drmFreeVersion(v);
        if (match)
            d->fd = fd;
        else
            close(fd);
    }
    if (d->fd < 0 || drmSetMaster(d->fd))
        return -1;
    drmModeRes *r = drmModeGetResources(d->fd);
    if (!r)
        return -1;
    for (int i = 0; i < r->count_connectors && !d->connector; i++) {
        drmModeConnector *c = drmModeGetConnector(d->fd, r->connectors[i]);
        if (c && c->connection == DRM_MODE_CONNECTED && c->count_modes) {
            d->connector = c->connector_id;
            d->mode = c->modes[0];
            for (int k = 0; k < c->count_modes; k++)
                if (c->modes[k].type & DRM_MODE_TYPE_PREFERRED) {
                    d->mode = c->modes[k];
                    break;
                }
            for (int k = 0; k < c->count_encoders && !d->crtc; k++) {
                drmModeEncoder *e = drmModeGetEncoder(d->fd, c->encoders[k]);
                if (e) {
                    d->crtc = e->crtc_id;
                    for (int j = 0; !d->crtc && j < r->count_crtcs; j++)
                        if (e->possible_crtcs & (1U << j))
                            d->crtc = r->crtcs[j];
                    drmModeFreeEncoder(e);
                }
            }
        }
        drmModeFreeConnector(c);
    }
    drmModeFreeResources(r);
    d->width = d->mode.hdisplay;
    d->height = d->mode.vdisplay;
    if (!d->crtc || d->width < 480 || d->width > 1024 || d->height < 360 || d->height > 1024)
        return -1;
    for (unsigned i = 0; i < 2; i++) {
        struct drm_mode_create_dumb create = {.width = d->width, .height = d->height, .bpp = 32};
        if (drmIoctl(d->fd, DRM_IOCTL_MODE_CREATE_DUMB, &create))
            return -1;
        d->handle = create.handle;
        d->pitch = create.pitch;
        d->size = create.size;
        if (d->pitch < d->width * 4 || d->size < (uint64_t)d->pitch * d->height ||
            d->size > 8 * 1024 * 1024)
            return -1;
        uint32_t handles[4] = {d->handle}, pitches[4] = {d->pitch}, offsets[4] = {0};
        if (drmModeAddFB2(d->fd, d->width, d->height, DRM_FORMAT_XRGB8888, handles, pitches, offsets,
                          &d->fbs[i], 0))
            return -1;
        struct drm_mode_map_dumb map = {.handle = d->handle};
        if (drmIoctl(d->fd, DRM_IOCTL_MODE_MAP_DUMB, &map))
            return -1;
        d->maps[i] = mmap(NULL, d->size, PROT_READ | PROT_WRITE, MAP_SHARED, d->fd, map.offset);
        if (d->maps[i] == MAP_FAILED)
            return -1;
    }
    d->back = 0;
    d->pixels = d->maps[0];
    d->fb = d->fbs[0];
    return 0;
}
static void light(const char *value) {
    int fd = open("/sys/class/backlight/y2-backlight/bl_power", O_WRONLY | O_CLOEXEC);
    if (fd < 0)
        return;
    if (write(fd, value, 2) != 2) {
    }
    close(fd);
}
/* As the boot splash: keep the light off across the first modeset (LK's
 * picture would show noise), then always turn it back on. */
static void swap_buffers(Display *d) {
    d->back ^= 1;
    d->pixels = d->maps[d->back];
    d->fb = d->fbs[d->back];
}
static void flipped(int fd, unsigned frame, unsigned sec, unsigned usec, void *data) {
    (void)fd, (void)frame, (void)sec, (void)usec;
    *(bool *)data = true;
}
/* Show the buffer just drawn at the next vblank and wait for it, so the
 * buffer drawn next is never the one on screen. */
static void present(Display *d) {
    bool done = false;
    if (!drmModePageFlip(d->fd, d->crtc, d->fb, DRM_MODE_PAGE_FLIP_EVENT, &done)) {
        drmEventContext events = {.version = 2, .page_flip_handler = flipped};
        struct pollfd pfd = {.fd = d->fd, .events = POLLIN};
        while (!done && poll(&pfd, 1, 200) > 0)
            drmHandleEvent(d->fd, &events);
    } else
        drmModeSetCrtc(d->fd, d->crtc, d->fb, 0, 0, &d->connector, 1, &d->mode);
    swap_buffers(d);
}
static int show(Display *d) {
    light("4\n");
    int ret = drmModeSetCrtc(d->fd, d->crtc, d->fb, 0, 0, &d->connector, 1, &d->mode);
    if (!ret) {
        drmVBlank vbl = {.request = {.type = DRM_VBLANK_RELATIVE, .sequence = 2}};
        if (drmWaitVBlank(d->fd, &vbl))
            poll(NULL, 0, 40);
    }
    light("0\n");
    if (!ret)
        swap_buffers(d);
    return ret;
}
static void rect(Display *d, unsigned x, unsigned y, unsigned w, unsigned h, uint32_t rgb) {
    if (x + w > d->width || y + h > d->height)
        return;
    for (unsigned row = y; row < y + h; row++) {
        uint32_t *p = (uint32_t *)(d->pixels + (size_t)row * d->pitch) + x;
        for (unsigned col = 0; col < w; col++)
            p[col] = rgb;
    }
}
static void text(Display *d, unsigned x, unsigned y, const char *s, uint32_t rgb) {
    for (; *s && x + FONT_W <= d->width; s++, x += FONT_W) {
        unsigned char c = (unsigned char)*s;
        if (c < FONT_FIRST || c >= FONT_FIRST + FONT_COUNT)
            c = '?';
        const unsigned char *g = font_ter16x32 + (c - FONT_FIRST) * 64;
        for (unsigned row = 0; row < FONT_H && y + row < d->height; row++) {
            uint32_t *p = (uint32_t *)(d->pixels + (size_t)(y + row) * d->pitch) + x;
            unsigned bits = (unsigned)g[row * 2] << 8 | g[row * 2 + 1];
            for (unsigned col = 0; col < FONT_W; col++)
                if (bits & (0x8000u >> col))
                    p[col] = rgb;
        }
    }
}
static void centered(Display *d, unsigned oy, unsigned y, const char *s, uint32_t rgb) {
    size_t n = strlen(s);
    if (n > COLUMNS)
        n = COLUMNS;
    char line[COLUMNS + 1];
    memcpy(line, s, n);
    line[n] = 0;
    text(d, (d->width - (unsigned)n * FONT_W) / 2, oy + y, line, rgb);
}
static bool slurp(const char *path, char *buf, size_t cap) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return false;
    ssize_t n = read(fd, buf, cap - 1);
    close(fd);
    if (n < 0)
        return false;
    buf[n] = 0;
    return true;
}
/* Installer steps (y2-update-core progress()) in words, and their place in
 * the four stages an owner sees ("Step 2 of 4"). Stage 0: no counter. */
static const char *step_text(const char *step, unsigned *stage) {
    static const struct { const char *step, *words; unsigned stage; } map[] = {
        {"verify", "Checking the update", 1},     {"backup", "Saving the current system", 2},
        {"system", "Installing the system", 3},   {"check", "Checking the system", 4},
        {"boot", "Installing the kernel", 4},     {"installed", "Update installed", 0},
        {"restore", "Restoring the old system", 0}, {"restored", "Old system restored", 0},
        {"probe", "Recovery works", 0},           {"done", "Nothing to install", 0},
    };
    for (size_t i = 0; i < sizeof(map) / sizeof(map[0]); i++)
        if (!strcmp(map[i].step, step)) {
            *stage = map[i].stage;
            return map[i].words;
        }
    *stage = 0;
    return "";
}
/* Body lines, the step with animated dots (the screen visibly lives while a
 * long step runs; a moving mark in the bar read as a second progress bar),
 * its counter, the bar and the time left. phase counts redraws. */
static void draw(Display *d, const char *screen, const char *progress, const char *time_line,
                 unsigned phase) {
    unsigned ox = (d->width - 480) / 2, oy = (d->height - 360) / 2;
    rect(d, 0, 0, d->width, d->height, BG);
    unsigned percent = 0;
    char step[32] = {0};
    bool running = sscanf(progress, "%u %31s", &percent, step) >= 1;
    char copy[1024];
    snprintf(copy, sizeof(copy), "%s", screen);
    /* Blank lines are spacing, so split exactly at each newline. With a bar
     * on screen the body keeps clear of the step lines below it. */
    char *line = copy;
    for (unsigned i = 0; *line && i < (running ? 4u : LINES); i++) {
        char *end = strchr(line, '\n');
        if (end)
            *end = 0;
        centered(d, oy, i ? 72 + i * 36 : 28, line, i ? DIM : TEXT);
        if (!end)
            break;
        line = end + 1;
    }
    if (!running)
        return;
    if (percent > 100)
        percent = 100;
    unsigned stage;
    const char *words = step_text(step, &stage);
    if (*words) {
        /* Center the words alone, so the dots do not shift them. */
        size_t n = strlen(words);
        unsigned x = (d->width - (unsigned)n * FONT_W) / 2;
        text(d, x, oy + 216, words, TEXT);
        if (stage) {
            static const char *const dots[] = {"", ".", "..", "..."};
            text(d, x + (unsigned)n * FONT_W, oy + 216, dots[(phase / 3) % 4], TEXT);
        }
    }
    if (stage) {
        char counter[24];
        snprintf(counter, sizeof(counter), "Step %u of 4", stage);
        centered(d, oy, 250, counter, DIM);
    }
    unsigned fill = 4 * percent;
    rect(d, ox + 40, oy + 292, 400, 6, TRACK);
    rect(d, ox + 40, oy + 292, fill, 6, FILL);
    if (time_line && *time_line)
        centered(d, oy, 314, time_line, DIM);
}
static uint64_t now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
/* The installer's own estimate (OTA-50: "<percent> <step> <seconds>"), in
 * fives under a minute like Reborn's download screen. */
static unsigned rounded_left(unsigned seconds) {
    return seconds < 10 ? 0 : seconds < 60 ? (seconds + 2) / 5 * 5 : (seconds + 30) / 60 * 60;
}
static void seconds_left(char *out, size_t cap, unsigned rounded) {
    if (rounded < 10)
        snprintf(out, cap, "A few seconds left");
    else if (rounded < 60)
        snprintf(out, cap, "About %u seconds left", rounded);
    else if (rounded < 120)
        snprintf(out, cap, "About a minute left");
    else
        snprintf(out, cap, "About %u min left", rounded / 60);
}
/* Without the installer's estimate (an older core): the average rate so far,
 * rounded up to whole minutes, once there is enough to go on. */
static void time_left(char *out, size_t cap, unsigned percent, uint64_t elapsed_ms) {
    if (percent >= 100) {
        out[0] = 0;
        return;
    }
    if (percent < 3 || elapsed_ms < 8000) {
        snprintf(out, cap, "Estimating time left");
        return;
    }
    uint64_t left = elapsed_ms * (100 - percent) / percent / 1000;
    if (left < 50)
        snprintf(out, cap, "Less than a minute left");
    else
        snprintf(out, cap, "About %u min left", (unsigned)((left + 59) / 60));
}
int main(void) {
    Display d;
    bool ready = false;
    /* The display module loads in the background (y2-platform-start). */
    for (unsigned attempt = 0; attempt < 300 && !ready; attempt++) {
        if (!display_open(&d))
            ready = true;
        else {
            if (d.fd >= 0)
                close(d.fd);
            poll(NULL, 0, 100);
        }
    }
    if (!ready)
        return 1;
    char screen[1024] = {0}, progress[64] = {0}, last_screen[1024] = {1}, last_progress[64] = {1};
    char line[40] = {0}, last_line[40] = {1}, reported_line[64] = {0};
    bool shown = false;
    uint64_t started = 0, refreshed = 0, reported_at = 0;
    unsigned phase = 0, reported = 0, displayed = UINT_MAX;
    for (;;) {
        if (!slurp("/run/y2/recovery-screen", screen, sizeof(screen)))
            screen[0] = 0;
        if (!slurp("/run/y2/recovery-progress", progress, sizeof(progress)))
            progress[0] = 0;
        unsigned percent = 0, seconds = 0;
        char step[32];
        int fields = sscanf(progress, "%u %31s %u", &percent, step, &seconds);
        bool running = fields >= 1 && percent < 100;
        uint64_t now = now_ms();
        if (running && !started)
            started = now;
        if (running && fields == 3) {
            /* Count down between the installer's reports; the shown value
             * only goes up when the estimate grew by ten seconds or more. */
            if (strcmp(progress, reported_line)) {
                snprintf(reported_line, sizeof(reported_line), "%s", progress);
                reported = seconds;
                reported_at = now;
            }
            uint64_t gone = (now - reported_at) / 1000;
            unsigned left = gone < reported ? reported - (unsigned)gone : 1;
            unsigned r = rounded_left(left);
            if (displayed == UINT_MAX || r < displayed || r >= displayed + 10)
                displayed = r;
            seconds_left(line, sizeof(line), displayed);
        } else if (running && now - refreshed >= 3000) {
            /* The estimate changes at most every three seconds. */
            time_left(line, sizeof(line), percent, now - started);
            refreshed = now;
        } else if (!running) {
            line[0] = 0;
            displayed = UINT_MAX;
        }
        bool changed = strcmp(screen, last_screen) || strcmp(progress, last_progress) ||
                       strcmp(line, last_line);
        if (changed || running) {
            draw(&d, screen, progress, line, phase++);
            if (!shown)
                shown = !show(&d);
            else
                present(&d);
            memcpy(last_screen, screen, sizeof(screen));
            memcpy(last_progress, progress, sizeof(progress));
            memcpy(last_line, line, sizeof(line));
        }
        poll(NULL, 0, running ? 100 : 200);
    }
}
