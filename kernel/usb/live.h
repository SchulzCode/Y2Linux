/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_LIVE_H
#define Y2_USB_LIVE_H
#include "../diagnostic/pwrap.h"
#define Y2_USB_LIVE_MAGIC 0x59325532U
enum y2_usb_stage {
    Y2_USB_OFF, Y2_USB_PREFLIGHT, Y2_USB_ATTACH, Y2_USB_SESSION,
    Y2_USB_REGISTER, Y2_USB_READY, Y2_USB_CONFIGURED, Y2_USB_STOPPED,
    Y2_USB_FAILED
};
struct y2_usb_live {
    unsigned magic;
    int result;
    unsigned stage, polls, chrdet, devctl, irqs, events, configured;
    /* Zero magic until a worker poll fails; retained after mappings are freed. */
    struct y2_pwrap_snapshot power_failure;
};
#endif
