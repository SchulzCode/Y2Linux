/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_SESSION_H
#define Y2_USB_SESSION_H
/* LK 81e092f6..81e0930c / FM usb_phy_poweron, after guarded wake.
 * PHY digital device inputs only. This never controls a VBUS supply. */
struct y2_session_io {
    void *context;
    unsigned (*read)(void *, unsigned);
    void (*write)(void *, unsigned, unsigned);
    void (*delay)(void *);
};
struct y2_session {
    unsigned before_c, before_d, written, devctl;
};
static inline int y2_session_start(const struct y2_session_io *io,struct y2_session *s)
{
    unsigned c,d,i;
    *s=(struct y2_session){.devctl=0x100};
    c=io->read(io->context,0x6c);d=io->read(io->context,0x6d);
    s->before_c=c;s->before_d=d;
    if(!(c & 2) || d) return -19;
    io->write(io->context,0x6c,c & ~0x10U);s->written=1;
    c=io->read(io->context,0x6c);
    io->write(io->context,0x6c,c | 0x2e);
    d=io->read(io->context,0x6d);
    io->write(io->context,0x6d,d | 0x3e);
    if((io->read(io->context,0x6c)&0x3e)!=0x2e ||
       (io->read(io->context,0x6d)&0x3e)!=0x3e) return -5;
    for(i=0;i<1000;++i) {
        /* 0x100 denotes the separately mapped MAC DEVCTL byte. */
        s->devctl=io->read(io->context,0x100);
        if((s->devctl & 0x9f)==0x98) return 0;
        if(s->devctl & 7) return -19;
        io->delay(io->context);
    }
    return -110;
}
/* Disconnect MAC first, then relinquish only digital inputs this code forced.
 * Restore their saved value bits without rewriting sampled analog trims. */
static inline void y2_session_end(const struct y2_session_io *io,struct y2_session *s)
{
    unsigned v;
    if(!s->written) return;
    v=io->read(io->context,0x6d);
    io->write(io->context,0x6d,(v & ~0x3eU) | (s->before_d & 0x3e));
    v=io->read(io->context,0x6c);
    io->write(io->context,0x6c,(v & ~0x3eU) | (s->before_c & 0x3e));
    s->written=0;
}
#endif
