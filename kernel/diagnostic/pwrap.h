/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PWRAP_H
#define Y2_PWRAP_H

/* Temporary M2-PWRAP-01 contract; see docs/knowledge/m2-pwrap-probe.md. */
#define Y2_PWRAP_BASE 0x1000d000U
#define Y2_PWRAP_BYTES 0xa8U
#define Y2_PWRAP_MAGIC 0x59325031U
#define Y2_PWRAP_POLLS 1000U
#define Y2_PWRAP_INIT (1U << 21)
#define Y2_PWRAP_SYNC (1U << 20)
#define Y2_PWRAP_REQ (1U << 19)
struct y2_pwrap_snapshot {
    unsigned magic;
    int result;
    unsigned valid, mux, wrap, arb, channel, init;
    unsigned before, after, cid, vusb;
};
struct y2_pwrap_io {
    void *context;
    unsigned (*read)(void *, unsigned);
    void (*write)(void *, unsigned, unsigned);
    void (*delay)(void *);
};
static inline unsigned y2_pwrap_fsm(unsigned state)
{ return (state >> 16) & 7U; }
static inline int y2_pwrap_idle(unsigned state)
{
    return (state & (Y2_PWRAP_INIT | Y2_PWRAP_SYNC | Y2_PWRAP_REQ)) ==
        (Y2_PWRAP_INIT | Y2_PWRAP_SYNC) && !y2_pwrap_fsm(state);
}
static inline int y2_pwrap_wait(const struct y2_pwrap_io *io,
                                unsigned *state, int completion)
{
    unsigned i, fsm;
    for (i = 0; i < Y2_PWRAP_POLLS; ++i) {
        *state = io->read(io->context, 0xa0);
        if (!(*state & Y2_PWRAP_INIT)) return -5;
        fsm = y2_pwrap_fsm(*state);
        if (completion ? fsm == 6 : y2_pwrap_idle(*state)) return 0;
        if (fsm != 0 && fsm != 2 && fsm != 4 && fsm != 6) return -5;
        io->delay(io->context); /* fixed 10us budget, no timer dependence */
    }
    return -110;
}
static inline int y2_pwrap_read_pmic(const struct y2_pwrap_io *io,
                                    unsigned address, unsigned *value,
                                    unsigned *state)
{
    int rc;
    /* No caller can use this helper for an arbitrary register or PMIC write. */
    if (address != 0x100 && address != 0x502) return -22;
    *state = io->read(io->context, 0xa0);
    if (!y2_pwrap_idle(*state)) return -16;
    io->write(io->context, 0x9c, (address >> 1) << 16);
    rc = y2_pwrap_wait(io, state, 1);
    if (rc) return rc; /* no stale clearing, reset, retry or guessed recovery */
    *value = *state & 0xffffU;
    io->write(io->context, 0xa4, 1); /* acknowledge only our completed read */
    return y2_pwrap_wait(io, state, 0);
}
static inline void y2_pwrap_probe(const struct y2_pwrap_io *io,
                                 struct y2_pwrap_snapshot *s)
{
    *s = (struct y2_pwrap_snapshot){ .magic = Y2_PWRAP_MAGIC, .result = -19 };
    s->mux = io->read(io->context, 0x00);
    s->wrap = io->read(io->context, 0x04);
    s->arb = io->read(io->context, 0x50);
    s->channel = io->read(io->context, 0x94);
    s->init = io->read(io->context, 0x98);
    s->before = s->after = io->read(io->context, 0xa0);
    /* MT6582 WACS2 arbitration bit is 3, not a sibling-derived value. */
    if (s->mux != 0 || s->wrap != 1 || !(s->arb & (1U << 3)) ||
        s->channel != 1 || s->init != 1) return;
    if (!y2_pwrap_idle(s->before)) { s->result = -16; return; }
    s->result = y2_pwrap_read_pmic(io, 0x100, &s->cid, &s->after);
    if (s->result) return;
    s->valid = 1;
    if ((s->cid & 0xffU) != 0x23) { s->result = -19; return; }
    s->result = y2_pwrap_read_pmic(io, 0x502, &s->vusb, &s->after);
    if (!s->result) s->valid |= 2;
}
#endif
