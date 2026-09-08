/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_DIAGNOSTIC_POLICY_H
#define Y2_DIAGNOSTIC_POLICY_H
/* D13: fixed LK scanout aperture, never allocator RAM. */
#define Y2_HEARTBEATS 10U
#define Y2_FB_BASE 0xbfb00000U
#define Y2_WIDTH 480U
#define Y2_HEIGHT 360U
#define Y2_STRIDE 960U
#define Y2_FB_BYTES (Y2_STRIDE * Y2_HEIGHT)
struct y2_snapshot {
    unsigned wdt_mode, ovl_en, src_con, roi, con, size, offset, addr, pitch;
    unsigned dsi_start, dsi_ctrl, dsi_mode;
};
static inline int y2_valid(const struct y2_snapshot *s)
{
    return !(s->wdt_mode & 1) && (s->ovl_en & 1) &&
        (s->src_con & 4) && s->roi == ((Y2_HEIGHT << 16) | Y2_WIDTH) &&
        /* Memory source, RGB565, no key/alpha or external color matrix. */
        (s->con & 0x70fff100U) == 0x1000U &&
        s->size == ((Y2_HEIGHT << 16) | Y2_WIDTH) && s->offset == 0 &&
        s->addr == Y2_FB_BASE && (s->pitch & 0xffff) == Y2_STRIDE &&
        (s->dsi_start & 1) && (s->dsi_ctrl & 3) == 2 &&
        /* Sync pulse video; no frame, mixed, switching or sleep mode. */
        (s->dsi_mode & 0x1f0003U) == 1;
}
static inline int y2_next_stage(unsigned phase, unsigned beats, char command)
{
    if (command == 'I' && phase == 0 && beats == 0) return 1;
    if (command == 'H' && phase == 1 && beats < Y2_HEARTBEATS) return 2 + (beats & 1);
    if (command == 'F' && phase == 1 && beats == Y2_HEARTBEATS) return 4;
    return -1;
}
/* 0 kernel stripes; 1 PID1 green; 2/3 heartbeat; 4 terminal checker. */
static inline unsigned short y2_pixel(unsigned stage, unsigned x, unsigned y)
{
    if (stage == 0) return (x / 40) & 1 ? 0xffff : 0;
    if (stage == 1) return 0x07e0;
    if (stage == 2 || stage == 3)
        return (x < Y2_WIDTH / 2) == (stage == 2) ? 0xffff : 0x07e0;
    return ((x / 40) ^ (y / 40)) & 1 ? 0xffff : 0;
}
#endif
