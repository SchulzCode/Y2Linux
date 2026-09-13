/* SPDX-License-Identifier: GPL-2.0-only */
/* Read cached kernel diagnostics only; no register, clock or storage writes. */
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
#include "../../kernel/usb/live.h"
#include "../../kernel/diagnostic/usb_state.h"

int main(int argc, char **argv)
{
    static const char *names[] = {"OFF", "PREFLIGHT", "ATTACH", "SESSION",
        "REGISTER", "READY", "CONFIGURED", "STOPPED", "DETACHED", "FAILED"};
    struct y2_usb_live live;
    struct y2_platform_snapshot initial;
    int fd = open(argc == 2 ? argv[1] : "/dev/y2diag", O_RDONLY | O_CLOEXEC);
    if (fd < 0) { printf("USB status open errno=%d\n", errno); return 1; }
    if (read(fd, &live, sizeof(live)) != sizeof(live) ||
        live.magic != Y2_USB_LIVE_MAGIC || live.stage >= sizeof(names)/sizeof(*names)) {
        puts("USB status invalid/short"); close(fd); return 2;
    }
    printf("USB %s rc=%d configured=%u\n", names[live.stage], live.result, live.configured);
    printf("Live CHR=%08x polls=%u IRQ=%u\n", live.chrdet, live.polls, live.irqs);
    printf("DEVCTL=%02x events=%u\n", live.devctl, live.events);
    if (read(fd, &initial, sizeof(initial)) != sizeof(initial) ||
        initial.power.magic != Y2_PWRAP_MAGIC) {
        puts("Initial snapshot invalid/short"); close(fd); return 2;
    }
    printf("Initial PW=%d/%u CHR=%04x\n", initial.power.result,
        initial.power.valid, initial.power.chrdet);
    printf("CLK=%d/%u USB=%d/%x WAKE=%d/%u\n", initial.clock.result,
        initial.clock.valid, initial.usb.result, initial.usb.valid,
        initial.wake.result, initial.wake.written);
    printf("MUX=%08x PLL=%08x PWR=%08x\n", initial.clock.mux,
        initial.clock.pll, initial.clock.pll_power);
    close(fd);
    return 0;
}
