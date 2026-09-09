/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_WAKE_H
#define Y2_USB_WAKE_H
#include "usb_state.h"
static const unsigned y2_usb_mode_offsets[7] = {0,5,0x15,0x1a,0x1d,0x22,0x63};
struct y2_usb_wake_io {
    void *context;
    int (*read)(void *, unsigned, unsigned, unsigned *);
    int (*release_suspend)(void *, unsigned *);
    void (*settle)(void *);
};
static inline void y2_usb_wake_probe(const struct y2_usb_wake_io *io,
                                    struct y2_platform_snapshot *s)
{
    struct y2_usb_wake_snapshot *w = &s->wake;
    const unsigned *v = s->usb.values;
    unsigned i;
    *w = (struct y2_usb_wake_snapshot){ .result = -19 };
    if (!y2_usb_state_ready(s) || s->usb.result || s->usb.valid != 0x1fffff ||
        v[0] != 0x20 || (v[1] & 0x87) != 0x80 || v[2] != 0x6503 ||
        v[6] || v[7] || v[8] != 4 || v[9] != 2 || !(v[10] & 2) || v[11] || v[12]) return;
    for (i = 13; i < 21; ++i) if (v[i]) return;
    for (i = 0; i < 7; ++i) {
        w->result = io->read(io->context,Y2_USB_PHY_BASE+y2_usb_mode_offsets[i],1,&w->controls[i]);
        if (w->result) return;
        w->before_valid |= 1U << i;
    }
    w->result = -19;
    if ((w->controls[2] & 0x80) || (w->controls[3] & 0x80) ||
        (w->controls[4] & 0x10) || (w->controls[5] & 3)) return;
    w->result = io->release_suspend(io->context,&w->written);
    if (w->result) return;
    io->settle(io->context);
    for (i = 0; i < 12; ++i) {
        unsigned address = i < 7 ? Y2_USB_PHY_BASE+0x68+i :
            i < 10 ? Y2_USB_PHY_BASE+y2_usb_mode_offsets[i-7] :
            i == 10 ? Y2_USB_MAC_BASE+1 : Y2_USB_MAC_BASE+0x60;
        w->result = io->read(io->context,address,1,&w->after[i]);
        if (w->result) return;
        w->after_valid |= 1U << i;
    }
    if (w->after[2] || w->after[7] != w->controls[0] ||
        w->after[8] != w->controls[1] || w->after[9] != w->controls[2] ||
        (w->after[10] & 0x40) || (w->after[11] & 0x87) != 0x80) w->result = -5;
}
#endif
