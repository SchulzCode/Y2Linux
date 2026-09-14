/* SPDX-License-Identifier: GPL-2.0-only */
/* Rescue-owned charging display. The kernel owns every charger operation.
 * No storage mounts, network, charger register IO, or fabricated percentage. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/fb.h>
#include <linux/input.h>
#include <linux/kd.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/reboot.h>
#include <time.h>
#include <unistd.h>

#define BAT "/sys/class/power_supply/BAT0/"
#define SOURCE "/sys/class/power_supply/y2-usb-presence/online"
#define LIGHT "/sys/class/backlight/y2-backlight/brightness"
#define BOOT_UV 3400000 /* stock precharge→CC threshold; margin over LK's 3.2V */
#define SCREEN_MS 8000
#define HOLD_MS 2000 /* below the untouched PMIC emergency long-press interval */

static int logfd = -1, fb = -1, tty = -1;
static struct fb_fix_screeninfo fix;
static struct fb_var_screeninfo var;
static unsigned char *pixels;
static unsigned saved_light = 8;
static bool lit;
static unsigned parked_cpus;
static int read_number(const char *path, int *value);
static int write_text(const char *path, const char *value);

static void cpu_policy(bool offline)
{
    for (unsigned n = 1; n < 4; n++) {
        char path[96];
        snprintf(path, sizeof(path), "/sys/devices/system/cpu/cpu%u/online", n);
        if (offline) {
            int online;
            if (!read_number(path, &online) && online == 1 &&
                !write_text(path, "0\n")) parked_cpus |= 1U << n;
        } else if (parked_cpus & (1U << n)) {
            if (write_text(path, "1\n") && logfd >= 0)
                dprintf(logfd, "Y2CHARGE: CPU%u restore failed\n", n);
        }
    }
}

static uint64_t milliseconds(void)
{
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) _exit(2);
    return (uint64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static int read_text(const char *path, char *buf, size_t bytes)
{
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = read(fd, buf, bytes - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    return 0;
}
static int read_number(const char *path, int *value)
{
    char buf[64], *end;
    long n;
    if (read_text(path, buf, sizeof(buf))) return -1;
    errno = 0;
    n = strtol(buf, &end, 10);
    if (errno || end == buf || (*end && *end != '\n') || n < 0 || n > 10000000)
        return -1;
    *value = n;
    return 0;
}
static int write_text(const char *path, const char *value)
{
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    size_t n = strlen(value);
    ssize_t written = write(fd, value, n);
    int ret = written == (ssize_t)n ? 0 : -1;
    if (close(fd)) ret = -1;
    return ret;
}
static void brightness(unsigned value)
{
    char buf[20];
    snprintf(buf, sizeof(buf), "%u\n", value);
    if (write_text(LIGHT, buf) && logfd >= 0)
        dprintf(logfd, "Y2CHARGE: backlight write failed\n");
}
static int display_open(void)
{
    if (pixels) return 0;
    fb = open("/dev/fb0", O_RDWR | O_CLOEXEC);
    if (fb < 0) return -1;
    if (ioctl(fb, FBIOGET_FSCREENINFO, &fix) || ioctl(fb, FBIOGET_VSCREENINFO, &var) ||
        var.xres != 480 || var.yres != 360 || (var.bits_per_pixel != 16 && var.bits_per_pixel != 32) ||
        fix.type != FB_TYPE_PACKED_PIXELS || fix.visual != FB_VISUAL_TRUECOLOR ||
        !fix.smem_len || fix.smem_len > 4 * 1024 * 1024 ||
        (uint64_t)(var.yoffset + var.yres) * fix.line_length > fix.smem_len ||
        (uint64_t)(var.xoffset + var.xres) * (var.bits_per_pixel / 8) > fix.line_length ||
        var.red.length > 8 || var.green.length > 8 || var.blue.length > 8 || var.transp.length > 8 ||
        var.red.offset + var.red.length > var.bits_per_pixel ||
        var.green.offset + var.green.length > var.bits_per_pixel ||
        var.blue.offset + var.blue.length > var.bits_per_pixel ||
        var.transp.offset + var.transp.length > var.bits_per_pixel)
        goto fail;
    pixels = mmap(NULL, fix.smem_len, PROT_READ | PROT_WRITE, MAP_SHARED, fb, 0);
    if (pixels == MAP_FAILED) { pixels = NULL; goto fail; }
    tty = open("/dev/tty0", O_RDWR | O_CLOEXEC);
    if (tty >= 0) ioctl(tty, KDSETMODE, KD_GRAPHICS);
    return 0;
fail:
    close(fb); fb = -1;
    return -1;
}
static unsigned component(unsigned v, struct fb_bitfield field)
{
    return field.length ? (v >> (8 - field.length)) << field.offset : 0;
}
static void rectangle(unsigned x, unsigned y, unsigned width, unsigned height, unsigned rgb)
{
    unsigned bytes = var.bits_per_pixel / 8;
    uint32_t pixel = component((rgb >> 16) & 255, var.red) |
        component((rgb >> 8) & 255, var.green) | component(rgb & 255, var.blue) |
        component(255, var.transp);
    if (!pixels || x + width > var.xres || y + height > var.yres) return;
    for (unsigned row = y; row < y + height; row++)
        for (unsigned col = x; col < x + width; col++) {
            size_t at = (row + var.yoffset) * fix.line_length + (col + var.xoffset) * bytes;
            for (unsigned k = 0; k < bytes; k++) pixels[at + k] = pixel >> (8 * k);
        }
}
/* A small original 5x7 uppercase alphabet; no UI toolkit or external font. */
static const uint8_t alphabet[26][7] = {
    {14,17,17,31,17,17,17},{30,17,17,30,17,17,30},{14,17,16,16,16,17,14},
    {30,17,17,17,17,17,30},{31,16,16,30,16,16,31},{31,16,16,30,16,16,16},
    {14,17,16,23,17,17,15},{17,17,17,31,17,17,17},{14,4,4,4,4,4,14},
    {7,2,2,2,2,18,12},{17,18,20,24,20,18,17},{16,16,16,16,16,16,31},
    {17,27,21,21,17,17,17},{17,25,21,19,17,17,17},{14,17,17,17,17,17,14},
    {30,17,17,30,16,16,16},{14,17,17,17,21,18,13},{30,17,17,30,20,18,17},
    {15,16,16,14,1,1,30},{31,4,4,4,4,4,4},{17,17,17,17,17,17,14},
    {17,17,17,17,17,10,4},{17,17,17,21,21,21,10},{17,17,10,4,10,17,17},
    {17,17,10,4,4,4,4},{31,1,2,4,8,16,31},
};
static void label(const char *text, unsigned y, unsigned scale, unsigned rgb)
{
    size_t length = strlen(text);
    if (!length || length * 6 * scale > 480) return;
    unsigned x = (480 - (length * 6 - 1) * scale) / 2;
    for (size_t i = 0; i < length; i++) {
        if (text[i] < 'A' || text[i] > 'Z') continue;
        for (unsigned row = 0; row < 7; row++)
            for (unsigned col = 0; col < 5; col++)
                if (alphabet[text[i] - 'A'][row] & (16 >> col))
                    rectangle(x + (i * 6 + col) * scale, y + row * scale, scale, scale, rgb);
    }
}
static void draw(const char *status, bool held, bool boot_ready, uint64_t now)
{
    bool charging = !strcmp(status, "Charging\n");
    bool full = !strcmp(status, "Full\n");
    unsigned accent = charging ? 0x55dba5 : 0xc2c8ce;
    rectangle(0, 0, 480, 360, 0x080e14);
    rectangle(151, 110, 170, 84, 0xc2c8ce);
    rectangle(157, 116, 158, 72, 0x080e14);
    rectangle(321, 135, 10, 34, 0xc2c8ce);
    /* Moving blocks indicate activity, never a claimed state of charge. */
    unsigned blocks = full ? 5 : charging ? 1 + (now / 500) % 5 : 1;
    for (unsigned i = 0; i < blocks; i++) rectangle(165 + i * 29, 124, 22, 56, accent);
    label(full ? "CHARGED" : charging ? "CHARGING" : "CHARGING PAUSED", 220, 3, 0xeef3f8);
    label(held ? (boot_ready ? "RELEASE TO START" : "LOW BATTERY") : "HOLD POWER TO START",
          281, 2, 0x9babbc);
}
static void screen(bool on)
{
    if (on == lit || fb < 0) return;
    if (on) {
        if (ioctl(fb, FBIOBLANK, FB_BLANK_UNBLANK) && logfd >= 0)
            dprintf(logfd, "Y2CHARGE: display unblank failed errno=%d\n", errno);
        brightness(saved_light);
    } else {
        brightness(0);
        if (ioctl(fb, FBIOBLANK, FB_BLANK_POWERDOWN) && logfd >= 0)
            dprintf(logfd, "Y2CHARGE: display blank failed errno=%d\n", errno);
    }
    lit = on;
}
static int power_input(void)
{
    for (unsigned n = 0; n < 32; n++) {
        char path[64], name[64] = {0};
        snprintf(path, sizeof(path), "/dev/input/event%u", n);
        int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        if (ioctl(fd, EVIOCGNAME(sizeof(name)), name) >= 0 && !strcmp(name, "mtk-pmic-keys"))
            return fd;
        close(fd);
    }
    return -1;
}
static void power_off(void)
{
    if (logfd >= 0) dprintf(logfd, "Y2CHARGE: source removed; hardware poweroff\n");
    screen(false);
    /* PID1 has mounted no root/data. Kernel shutdown inhibits charging first. */
    sync();
    reboot(RB_POWER_OFF);
    /* Never fall through to normal boot if the PMIC did not turn off. */
    for (;;) poll(NULL, 0, 1000);
}
static void display_close(void)
{
    cpu_policy(false);
    screen(true);
    rectangle(0, 0, 480, 360, 0);
    if (tty >= 0) { ioctl(tty, KDSETMODE, KD_TEXT); close(tty); }
    if (pixels) munmap(pixels, fix.smem_len);
    if (fb >= 0) close(fb);
}
/* A new press and release are required after input loss or a new open.
 * Source/presence/voltage are checked separately, freshly, before handover. */
struct power_interaction {
    bool pressed, dropped;
    uint64_t down, show_until;
};
static bool power_event(struct power_interaction *p, const struct input_event *e, uint64_t now)
{
    if (e->type == EV_SYN && e->code == SYN_DROPPED) {
        p->pressed = false; p->dropped = true;
        return false;
    }
    if (p->dropped) {
        if (e->type == EV_SYN && e->code == SYN_REPORT) p->dropped = false;
        return false;
    }
    if (e->type != EV_KEY || e->code != KEY_POWER) return false;
    if (e->value == 1) {
        p->pressed = true; p->down = now; p->show_until = now + SCREEN_MS;
    } else if (e->value == 0) {
        bool start = p->pressed && now - p->down >= HOLD_MS;
        p->pressed = false; p->show_until = now + SCREEN_MS;
        return start;
    }
    return false;
}
static bool safe_boot_sample(int source, int present, int voltage)
{
    return source == 1 && present == 1 && voltage >= BOOT_UV;
}
int main(void)
{
    char metadata[160], status[64] = "Unknown\n";
    int valid, mode, reason, offline, online = -1, uv = 0, present = 0, light = 0;
    int input = -1, good_voltage = 0;
    bool cpu_policy_applied = false;
    struct power_interaction power = {0};
    uint64_t next_sample = 0, next_frame = 0;
    logfd = open("/dev/kmsg", O_WRONLY | O_CLOEXEC);
    if (read_text("/sys/firmware/y2_boot/metadata", metadata, sizeof(metadata)) ||
        sscanf(metadata, "valid=%d boot_mode=%d boot_reason=%d offline=%d", &valid, &mode, &reason, &offline) != 4 ||
        valid != 1) {
        if (logfd >= 0) dprintf(logfd, "Y2CHARGE: missing valid loader metadata; rescue required\n");
        return 2;
    }
    /* Wait boundedly for normal provider probing, not for the battery to fill. */
    for (unsigned retry = 0; retry < 50; retry++) {
        if (!read_number(SOURCE, &online) && !read_number(BAT "present", &present) &&
            !read_number(BAT "voltage_now", &uv) && uv > 0 && present == 1) break;
        poll(NULL, 0, 100);
    }
    if (online < 0 || !present || uv <= 0) return 2;
    if (!offline && uv >= BOOT_UV) return 0;
    if (!online) power_off();
    if (logfd >= 0) dprintf(logfd, "Y2CHARGE: offline mode=%d reason=%d battery=%duV; no switch_root\n", mode, reason, uv);
    if (!read_number(LIGHT, &light) && light > 0) saved_light = light > 8 ? 8 : light;
    /* Policy is already powersave at boot. Keep it explicit in the small
     * charging environment; no service or SSH connection keeps charging alive. */
    write_text("/sys/devices/system/cpu/cpufreq/policy0/scaling_governor", "powersave\n");
    power.show_until = milliseconds() + SCREEN_MS;
    for (;;) {
        uint64_t now = milliseconds();
        if (now >= next_sample) {
            int source, voltage, battery_present;
            if (!read_number(SOURCE, &source)) {
                online = source;
                if (!online) power_off();
            }
            if (!read_number(BAT "voltage_now", &voltage) &&
                !read_number(BAT "present", &battery_present) && safe_boot_sample(online, battery_present, voltage))
                good_voltage = good_voltage < 3 ? good_voltage + 1 : 3;
            else good_voltage = 0;
            if (read_text(BAT "status", status, sizeof(status))) strcpy(status, "Unknown\n");
            if (fb < 0 && !display_open()) {
                /* A late panel probe still gets a full initial display period. */
                power.show_until = now + SCREEN_MS;
                lit = false;
            }
            /* Display initialization is pinned to CPU1. Wait for its actual
             * completion before parking secondary CPUs, including a late probe. */
            if (pixels && !cpu_policy_applied) {
                char display_state[128];
                if (!read_text("/run/y2-display-state", display_state, sizeof(display_state)) &&
                    strstr(display_state, "module result=0 errno=0")) {
                    cpu_policy(true);
                    cpu_policy_applied = true;
                }
            }
            if (input < 0) { input = power_input(); power.pressed = false; power.dropped = false; }
            next_sample = now + 1000;
        }
        bool held = power.pressed && now - power.down >= HOLD_MS;
        if (pixels && (now < power.show_until || power.pressed)) {
            if (!lit || now >= next_frame) {
                draw(status, held, good_voltage >= 3, now);
                next_frame = now + 500;
            }
            screen(true);
        } else screen(false);
        struct pollfd pfd = { .fd = input, .events = POLLIN };
        int ready = poll(&pfd, input < 0 ? 0 : 1, lit ? 100 : 250);
        if (ready < 0 && errno != EINTR) return 2;
        if (ready > 0 && (pfd.revents & (POLLERR | POLLHUP | POLLNVAL))) {
            close(input); input = -1; power.pressed = false;
            continue;
        }
        if (ready > 0 && (pfd.revents & POLLIN)) {
            struct input_event events[16];
            ssize_t bytes = read(input, events, sizeof(events));
            if (bytes <= 0 || bytes % sizeof(events[0])) continue;
            for (unsigned n = 0; n < (unsigned)bytes / sizeof(events[0]); n++) {
                struct input_event *e = &events[n];
                if (power_event(&power, e, milliseconds()) && good_voltage >= 3) {
                    int source, voltage, battery_present;
                    /* Unplug wins over a Power release in the polling gap. */
                    if (read_number(SOURCE, &source)) continue;
                    if (!source) power_off();
                    if (read_number(BAT "voltage_now", &voltage) ||
                        read_number(BAT "present", &battery_present) ||
                        !safe_boot_sample(source, battery_present, voltage)) continue;
                    if (logfd >= 0) dprintf(logfd, "Y2CHARGE: deliberate Power release; normal boot\n");
                    display_close(); close(input);
                    return 0;
                }
            }
        }
    }
}
