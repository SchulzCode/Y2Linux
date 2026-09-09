/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_GATE_H
#define Y2_USB_GATE_H
#include "../diagnostic/usb_state.h"
/* Fresh snapshot after the independently checked 6a=04 -> 00 wake.
 * This predicate grants no access on incomplete power/clock/MAC evidence. */
static inline int y2_usb_takeover_ready(const struct y2_platform_snapshot *s)
{
    const unsigned *v=s->usb.values;
    if(!y2_usb_state_ready(s) || s->usb.result || s->usb.valid!=0x1fffff ||
       v[0]!=0x20 || (v[1]&0x87)!=0x80 || v[2]!=0x6503 ||
       v[6] || v[7] || v[8] || v[9]!=2 || !(v[10]&2) || v[11] || v[12]) return 0;
    for(unsigned i=13;i<21;++i) if(v[i]) return 0;
    return 1;
}
#endif
