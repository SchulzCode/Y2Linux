/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_RECOVER_H
#define Y2_USB_RECOVER_H
#include "../diagnostic/usb_wake.h"
/* MT6582 usb_phy_recover, digital recovery prefix only. See
 * docs/knowledge/storage03-corrections.md. No oscillator/trim/PLL writes. */
struct y2_recover_io {
    struct y2_usb_state_io state;
    int (*write)(void *, unsigned, unsigned);
    void (*delay)(void *, unsigned);
};
static inline int y2_usb_recovered_ready(const struct y2_platform_snapshot *s)
{
    const unsigned *v=s->usb.values;
    if(!y2_usb_state_ready(s) || s->usb.result || s->usb.valid!=0x1fffff ||
       (v[0]&0x40) || (v[1]&4) || v[2]!=0x6503 ||
       (v[6]&0xf4) || (v[7]&0x3c) || (v[8]&0xbe) ||
       (v[9]&4) || (v[11]&0x3e) || (v[12]&1)) return 0;
    for(unsigned i=13;i<21;++i) if(v[i]&1) return 0;
    return 1;
}
static inline void y2_usb_recover(const struct y2_recover_io *io,
                                 struct y2_platform_snapshot *s)
{
    static const unsigned steps[][3]={
        {0x1d,0x10,0},{0x6b,4,0},{0x6e,1,0},{0x6a,4,0},
        {0x68,0x40,0},{0x68,0x80,0},{0x68,0x30,0},{0x68,4,0},
        {0x69,0x3c,0},{0x6a,0x10,0},{0x6a,0x20,0},{0x6a,8,0},
        {0x6a,2,0},{0x6a,0x80,0},{0x1a,0x80,0},{0x1a,0,0x10},{0x6d,0x3e,0}
    };
    struct y2_usb_wake_snapshot *w=&s->wake;
    struct y2_platform_snapshot after=*s;
    const unsigned *v=s->usb.values;
    unsigned i,value,check;
    *w=(struct y2_usb_wake_snapshot){.result=-19};
    /* Board is peripheral-only. Validate resources and active DMA/host ownership,
     * not a particular loader's digital register snapshot. Dormant DMA channel
     * configuration and inherited device-mode SOFTCONN are normal handoff states. */
    if(!y2_usb_state_ready(s) || s->usb.result || s->usb.valid!=0x1fffff ||
       (v[1]&4) || v[2]!=0x6503) return;
    for(i=13;i<21;++i) if(v[i]&1) {w->result=-16;return;}
    for(i=0;i<7;++i) {
        w->result=io->state.read(io->state.context,Y2_USB_PHY_BASE+y2_usb_mode_offsets[i],1,&w->controls[i]);
        if(w->result) return;
        w->before_valid|=1U<<i;
    }
    w->result=-19;
    /* Disconnect before replacing digital PHY inputs. Never enable host VBUS
     * or an inherited DMA channel. Mask interrupt delivery before ownership. */
    static const unsigned quiesce[][2] = {
        {Y2_USB_MAC_BASE+0xa4,0}, {Y2_USB_MAC_BASE+0x0b,0},
        {Y2_USB_MAC_BASE+0x06,0}, {Y2_USB_MAC_BASE+0x08,0},
        {Y2_USB_MAC_BASE+0x01,0}, {Y2_USB_MAC_BASE+0x60,0}
    };
    for(i=0;i<sizeof(quiesce)/sizeof(quiesce[0]);++i) {
        w->result=io->write(io->state.context,quiesce[i][0],quiesce[i][1]);
        if(w->result) return;
        w->written=1;
    }
    io->delay(io->state.context,50);
    for(i=0;i<sizeof(steps)/sizeof(steps[0]);++i) {
        unsigned addr=Y2_USB_PHY_BASE+steps[i][0];
        w->result=io->state.read(io->state.context,addr,1,&value);
        if(w->result) return;
        value=(value&~steps[i][1])|steps[i][2];
        w->result=io->write(io->state.context,addr,value);
        if(w->result) return;
        w->written=1;
        w->result=io->state.read(io->state.context,addr,1,&check);
        if(w->result) return;
        if(check!=value) {w->result=-5;return;}
    }
    io->delay(io->state.context,800);
    y2_usb_state_probe(&io->state,&after);
    if(!y2_usb_recovered_ready(&after)) {w->result=-5;return;}
    for(i=0;i<7;++i) w->after[i]=after.usb.values[6+i];
    for(i=0;i<7;++i) {
        w->result=io->state.read(io->state.context,Y2_USB_PHY_BASE+y2_usb_mode_offsets[i],1,&value);
        if(w->result) return;
        check=w->controls[i];
        if(i==3) check=(check&~0x80U)|0x10;
        if(i==4) check&=~0x10U;
        if(value!=check) {w->result=-5;return;}
        w->controls[i]=value; /* registration verifies recovered controls */
        if(i<3) w->after[7+i]=value;
    }
    w->after[10]=after.usb.values[0];w->after[11]=after.usb.values[1];
    w->after_valid=0xfff;w->result=0;
}
#endif
