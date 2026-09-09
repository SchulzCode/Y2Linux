/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_STATE_H
#define Y2_USB_STATE_H
#include "usb_clock.h"
#define Y2_USB_MAC_BASE 0x11200000U
#define Y2_USB_MAC_BYTES 0x280U
#define Y2_USB_PHY_BASE 0x11210800U
#define Y2_USB_PHY_BYTES 0x70U
#define Y2_USB_STATE_COUNT 21U
struct y2_usb_state_snapshot {
    int result;
    unsigned valid, values[Y2_USB_STATE_COUNT];
};
struct y2_usb_wake_snapshot {
    int result;
    unsigned written, before_valid, after_valid, controls[7], after[12];
};
struct y2_platform_snapshot {
    struct y2_pwrap_snapshot power;
    struct y2_usb_clock_snapshot clock;
    struct y2_usb_state_snapshot usb;
    struct y2_usb_wake_snapshot wake;
};
struct y2_usb_state_io {
    void *context;
    int (*read)(void *, unsigned, unsigned, unsigned *);
};
struct y2_usb_register { unsigned address, width; };
static const struct y2_usb_register y2_usb_registers[Y2_USB_STATE_COUNT] = {
    {0x11200001,1}, {0x11200060,1}, {0x1120006c,2},
    {0x11200006,2}, {0x11200008,2}, {0x1120000b,1},
    {0x11210868,1}, {0x11210869,1}, {0x1121086a,1}, {0x1121086b,1},
    {0x1121086c,1}, {0x1121086d,1}, {0x1121086e,1},
    {0x11200204,2}, {0x11200214,2}, {0x11200224,2}, {0x11200234,2},
    {0x11200244,2}, {0x11200254,2}, {0x11200264,2}, {0x11200274,2},
};
static inline int y2_usb_state_ready(const struct y2_platform_snapshot *s)
{
    return s->power.magic == Y2_PWRAP_MAGIC && !s->power.result &&
        s->power.valid == 7 && (s->power.cid & 0xffU) == 0x23 &&
        (s->power.vusb & (1U << 15)) && !s->clock.result && s->clock.valid == 15 &&
        !(s->clock.peri & (1U << 10)) &&
        (s->clock.mux & 0x00870000U) == 0x00010000U &&
        (s->clock.pll & 0x05000001U) == 0x05000001U &&
        (s->clock.pll_power & 3U) == 1U;
}
static inline void y2_usb_state_probe(const struct y2_usb_state_io *io,
                                     struct y2_platform_snapshot *s)
{
    unsigned i;
    s->usb = (struct y2_usb_state_snapshot){ .result = -19 };
    if (!y2_usb_state_ready(s)) return;
    for (i = 0; i < Y2_USB_STATE_COUNT; ++i) {
        s->usb.result = io->read(io->context, y2_usb_registers[i].address,
                                y2_usb_registers[i].width, &s->usb.values[i]);
        if (s->usb.result) return;
        s->usb.valid |= 1U << i;
    }
}
#endif
