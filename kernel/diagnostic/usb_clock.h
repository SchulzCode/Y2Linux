/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_CLOCK_H
#define Y2_USB_CLOCK_H
#include "pwrap.h"
struct y2_usb_clock_snapshot {
    int result;
    unsigned valid, peri, mux, pll, pll_power;
};
struct y2_usb_clock_io {
    void *context;
    int (*read)(void *, unsigned, unsigned *);
};
static inline void y2_usb_clock_probe(const struct y2_usb_clock_io *io,
                                      const struct y2_pwrap_snapshot *power,
                                      struct y2_usb_clock_snapshot *s)
{
    static const unsigned addresses[] = {
        0x10003018U, 0x10000060U, 0x10209220U, 0x1020922cU,
    };
    unsigned *values[] = { &s->peri, &s->mux, &s->pll, &s->pll_power };
    unsigned i;
    *s = (struct y2_usb_clock_snapshot){ .result = -19 };
    if (power->magic != Y2_PWRAP_MAGIC || power->result || power->valid != 3 ||
        (power->cid & 0xffU) != 0x23 || !(power->vusb & (1U << 15))) return;
    for (i = 0; i < 4; ++i) {
        s->result = io->read(io->context, addresses[i], values[i]);
        if (s->result) return;
        s->valid |= 1U << i;
    }
}
#endif
