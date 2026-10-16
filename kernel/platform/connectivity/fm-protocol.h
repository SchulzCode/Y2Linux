/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_FM_PROTOCOL_H
#define Y2_FM_PROTOCOL_H
/* MT6627 FM (MT6582 CONSYS) wire format on STP channel 1. Packet builders,
 * register sequences and channel tables follow MediaTek's GPL mt6627 driver
 * (donorSource fmradio, artificery 53fb57bb); the receive subset and every
 * packet are checked byte-for-byte against the stock Y2 v3.2.0 kernel's own
 * functions run in an emulator (tests/test_fm.py). The stock Y2 has no chip
 * seek/scan: stations are found with soft-mute tune and the CQI decision.
 * Pure helpers shared with host tests: no firmware bytes, no allocation. */
#define Y2_FM_CHANNEL 1
#define Y2_FM_TX_MAX 1024
#define Y2_FM_PATCH_SEG 512
#define Y2_FM_CQI_MAX 96
#define Y2_FM_KHZ_MIN 87500
#define Y2_FM_KHZ_MAX 108000
#define Y2_FM_KHZ_STEP 50

/* Command opcodes (fm_link.h) and the event bits their replies raise. */
enum {
	Y2_FM_OP_FSPI_READ = 0x03, Y2_FM_OP_FSPI_WRITE = 0x04, Y2_FM_OP_ENABLE = 0x07,
	Y2_FM_OP_TUNE = 0x09, Y2_FM_OP_RDS_DATA = 0x0d, Y2_FM_OP_RAMPDOWN = 0x0e,
	Y2_FM_OP_SET_BITS = 0x11, Y2_FM_OP_PATCH = 0x12, Y2_FM_OP_COEFF = 0x13,
	Y2_FM_OP_SOFT_MUTE_TUNE = 0x17, Y2_FM_OP_HOST_READ = 0x18, Y2_FM_OP_HOST_WRITE = 0x19,
	Y2_FM_OP_CSPI_WRITE = 0x20, Y2_FM_OP_CSPI_READ = 0x21,
};
#define Y2_FM_EV(op) (1U << (op))
#define Y2_FM_EV_CSPI_READ (1U << 22)
#define Y2_FM_EV_CSPI_WRITE (1U << 26)
#define Y2_FM_EV_TUNE_DONE (1U << 28)

/* Main registers used by the receive path (mt6627_fm_reg.h). */
#define Y2_FM_REG_CHIP_ID 0x62
#define Y2_FM_REG_CHANNEL 0x65
#define Y2_FM_REG_RSSI 0x6c
#define Y2_FM_REG_INTR 0x69
#define Y2_FM_REG_INTRMASK 0x6a
#define Y2_FM_REG_EXTINTRMASK 0x6b
#define Y2_FM_REG_VOLUME 0x7d
#define Y2_FM_REG_I2S_TX 0x9b
#define Y2_FM_REG_MUTE 0x9c
#define Y2_FM_HOST_I2S 0x80101054U
#define Y2_FM_HOST_CSPI 0x8013000cU

struct y2_fm_pkt { unsigned char data[Y2_FM_TX_MAX]; unsigned size; };

static inline void y2_fm_begin(struct y2_fm_pkt *p, unsigned op)
{
	p->data[0] = 0x01; p->data[1] = op; p->size = 4;
}
static inline unsigned y2_fm_end(struct y2_fm_pkt *p)
{
	p->data[2] = (p->size - 4) & 0xff; p->data[3] = (p->size - 4) >> 8;
	return p->size;
}
static inline void y2_fm_put(struct y2_fm_pkt *p, unsigned value, unsigned bytes)
{
	while (bytes--) { p->data[p->size++] = value & 0xff; value >>= 8; }
}
/* Basic operations inside one ENABLE/RAMPDOWN batch (fm_bop_*). */
static inline void y2_fm_bop_write(struct y2_fm_pkt *p, unsigned addr, unsigned value)
{
	y2_fm_put(p, 0x80, 1); y2_fm_put(p, 3, 1); y2_fm_put(p, addr, 1); y2_fm_put(p, value, 2);
}
static inline void y2_fm_bop_udelay(struct y2_fm_pkt *p, unsigned us)
{
	y2_fm_put(p, 0x81, 1); y2_fm_put(p, 4, 1); y2_fm_put(p, us, 4);
}
static inline void y2_fm_bop_rd_until(struct y2_fm_pkt *p, unsigned addr, unsigned mask, unsigned value)
{
	y2_fm_put(p, 0x82, 1); y2_fm_put(p, 5, 1); y2_fm_put(p, addr, 1);
	y2_fm_put(p, mask, 2); y2_fm_put(p, value, 2);
}
static inline void y2_fm_bop_modify(struct y2_fm_pkt *p, unsigned addr, unsigned and_mask, unsigned or_mask)
{
	y2_fm_put(p, 0x83, 1); y2_fm_put(p, 5, 1); y2_fm_put(p, addr, 1);
	y2_fm_put(p, and_mask, 2); y2_fm_put(p, or_mask, 2);
}
static inline void y2_fm_bop_top_write(struct y2_fm_pkt *p, unsigned addr, unsigned value)
{
	y2_fm_put(p, 0x85, 1); y2_fm_put(p, 7, 1); y2_fm_put(p, 4, 1);
	y2_fm_put(p, addr, 2); y2_fm_put(p, value, 4);
}
static inline void y2_fm_bop_top_rd_until(struct y2_fm_pkt *p, unsigned addr, unsigned mask, unsigned value)
{
	y2_fm_put(p, 0x86, 1); y2_fm_put(p, 11, 1); y2_fm_put(p, 4, 1);
	y2_fm_put(p, addr, 2); y2_fm_put(p, mask, 4); y2_fm_put(p, value, 4);
}

/* Single register commands. */
static inline unsigned y2_fm_read_pkt(struct y2_fm_pkt *p, unsigned addr)
{
	y2_fm_begin(p, Y2_FM_OP_FSPI_READ); y2_fm_put(p, addr, 1); return y2_fm_end(p);
}
static inline unsigned y2_fm_write_pkt(struct y2_fm_pkt *p, unsigned addr, unsigned value)
{
	y2_fm_begin(p, Y2_FM_OP_FSPI_WRITE); y2_fm_put(p, addr, 1); y2_fm_put(p, value, 2);
	return y2_fm_end(p);
}
/* mt6627_set_bits(addr, bits, mask): reg = (reg & mask) | bits. */
static inline unsigned y2_fm_set_bits_pkt(struct y2_fm_pkt *p, unsigned addr, unsigned bits, unsigned mask)
{
	y2_fm_begin(p, Y2_FM_OP_SET_BITS); y2_fm_bop_modify(p, addr, mask, bits); return y2_fm_end(p);
}
static inline unsigned y2_fm_top_write_pkt(struct y2_fm_pkt *p, unsigned addr, unsigned value)
{
	y2_fm_begin(p, Y2_FM_OP_CSPI_WRITE); y2_fm_put(p, 4, 1); y2_fm_put(p, addr, 2);
	y2_fm_put(p, value, 4); return y2_fm_end(p);
}
static inline unsigned y2_fm_host_write_pkt(struct y2_fm_pkt *p, unsigned addr, unsigned value)
{
	y2_fm_begin(p, Y2_FM_OP_HOST_WRITE); y2_fm_put(p, addr, 4); y2_fm_put(p, value, 4);
	return y2_fm_end(p);
}
static inline unsigned y2_fm_host_read_pkt(struct y2_fm_pkt *p, unsigned addr)
{
	y2_fm_begin(p, Y2_FM_OP_HOST_READ); y2_fm_put(p, addr, 4); return y2_fm_end(p);
}
/* Firmware segment: [segments, index, bytes...]; the caller owns the bytes. */
static inline unsigned y2_fm_segment_pkt(struct y2_fm_pkt *p, unsigned op, unsigned count,
					 unsigned index, const unsigned char *data, unsigned size)
{
	if (size > Y2_FM_TX_MAX - 6) return 0;
	y2_fm_begin(p, op); y2_fm_put(p, count, 1); y2_fm_put(p, index, 1);
	for (unsigned i = 0; i < size; i++) p->data[p->size++] = data[i];
	return y2_fm_end(p);
}
/* Vendor segmentation: len/512 + 1 segments, the last one len%512 bytes. */
static inline unsigned y2_fm_segments(unsigned size) { return size / Y2_FM_PATCH_SEG + 1; }
static inline unsigned y2_fm_segment_size(unsigned size, unsigned index)
{
	return index + 1 < y2_fm_segments(size) ? Y2_FM_PATCH_SEG : size % Y2_FM_PATCH_SEG;
}

/* Power-up steps 1, 4 and 5 and the power-down batch (mt6627_fm_cmd.c). */
static inline unsigned y2_fm_clock_on_pkt(struct y2_fm_pkt *p, unsigned deemphasis)
{
	y2_fm_begin(p, Y2_FM_OP_ENABLE);
	y2_fm_bop_top_write(p, 0xa10, 0xffffffff);
	y2_fm_bop_top_write(p, 0x60, 0x30);
	y2_fm_bop_top_write(p, 0x60, 0x35);
	y2_fm_bop_top_rd_until(p, 0x60, 0xa, 0xa);
	y2_fm_bop_top_write(p, 0x60, 0x15);
	y2_fm_bop_top_write(p, 0x60, 0x05);
	y2_fm_bop_udelay(p, 10);
	y2_fm_bop_top_write(p, 0x60, 0x45);
	y2_fm_bop_top_write(p, 0x68, 0x3f);
	y2_fm_bop_top_write(p, 0x50, 0x1);
	y2_fm_bop_udelay(p, 3000);
	y2_fm_bop_top_write(p, 0x50, 0x3);
	y2_fm_bop_top_write(p, 0x50, 0x7);
	y2_fm_bop_top_write(p, 0x000, 0x0);
	y2_fm_bop_write(p, 0x70, 0x0040);
	y2_fm_bop_modify(p, 0x61, 0xefff, (deemphasis & 1) << 12);
	return y2_fm_end(p);
}
static inline unsigned y2_fm_digital_init_pkt(struct y2_fm_pkt *p)
{
	static const unsigned short writes[][2] = {
		{0x21, 0xe000}, {0xd8, 0x03f0}, {0xd9, 0x3f04}, {0xda, 0x0014},
		{0xdb, 0x2a38}, {0x23, 0x4000}, {0x6a, 0x0021}, {0x6b, 0x0021},
	};
	y2_fm_begin(p, Y2_FM_OP_ENABLE);
	for (unsigned i = 0; i < sizeof(writes) / sizeof(writes[0]); i++)
		y2_fm_bop_write(p, writes[i][0], writes[i][1]);
	y2_fm_bop_top_write(p, 0x50, 0xf);
	y2_fm_bop_modify(p, 0x61, 0xfffd, 0x0002);
	y2_fm_bop_modify(p, 0x61, 0xfffe, 0x0000);
	y2_fm_bop_udelay(p, 100000);
	y2_fm_bop_rd_until(p, 0x64, 0x001f, 0x0002);
	return y2_fm_end(p);
}
static inline unsigned y2_fm_fine_tune_pkt(struct y2_fm_pkt *p)
{
	static const unsigned short writes[][2] = {
		{0x01, 0xbee8}, {0x03, 0xf6ed}, {0x15, 0x0d80}, {0x16, 0x0068},
		{0x17, 0x092a}, {0x34, 0x807f}, {0x35, 0x311e},
	};
	y2_fm_begin(p, Y2_FM_OP_ENABLE);
	y2_fm_bop_top_write(p, 0x50, 0x7);
	for (unsigned i = 0; i < sizeof(writes) / sizeof(writes[0]); i++)
		y2_fm_bop_write(p, writes[i][0], writes[i][1]);
	y2_fm_bop_top_write(p, 0x50, 0xf);
	return y2_fm_end(p);
}
static inline unsigned y2_fm_power_down_pkt(struct y2_fm_pkt *p)
{
	y2_fm_begin(p, Y2_FM_OP_ENABLE);
	y2_fm_bop_modify(p, 0x9b, 0xfff8, 0);
	y2_fm_bop_top_write(p, 0x50, 0x330f);
	y2_fm_bop_write(p, 0x61, 0x0001);
	for (unsigned i = 0; i < 4; i++) y2_fm_bop_modify(p, 0x6e, 0xfff8, 0);
	y2_fm_bop_top_write(p, 0x50, 0x0);
	y2_fm_bop_top_write(p, 0x50, 0x4000);
	y2_fm_bop_top_write(p, 0x50, 0x0);
	y2_fm_bop_top_write(p, 0x60, 0x5);
	y2_fm_bop_top_write(p, 0x60, 0x15);
	y2_fm_bop_top_write(p, 0x60, 0x35);
	y2_fm_bop_top_write(p, 0x60, 0x30);
	y2_fm_bop_top_rd_until(p, 0x60, 0xa, 0x0);
	return y2_fm_end(p);
}
static inline unsigned y2_fm_rampdown_pkt(struct y2_fm_pkt *p)
{
	y2_fm_begin(p, Y2_FM_OP_RAMPDOWN);
	y2_fm_bop_modify(p, 0x63, 0xfff0, 0);
	y2_fm_bop_modify(p, 0x63, 0xffff, 0x0100);
	y2_fm_bop_rd_until(p, 0x69, 0x0001, 0x0001);
	y2_fm_bop_modify(p, 0x63, 0xfeff, 0);
	y2_fm_bop_modify(p, 0x69, 0xffff, 0x0001);
	return y2_fm_end(p);
}
/* The tune trigger; frequency and channel parameters are set before it. */
static inline unsigned y2_fm_tune_pkt(struct y2_fm_pkt *p)
{
	y2_fm_begin(p, Y2_FM_OP_TUNE); y2_fm_bop_modify(p, 0x63, 0xfff8, 0x0001); return y2_fm_end(p);
}
static inline unsigned y2_fm_soft_mute_tune_pkt(struct y2_fm_pkt *p, unsigned freq)
{
	y2_fm_begin(p, Y2_FM_OP_SOFT_MUTE_TUNE); y2_fm_put(p, 1, 1); y2_fm_put(p, freq, 2);
	return y2_fm_end(p);
}

/* Frequencies: the chip works in 10 kHz units (8750 = 87.50 MHz). */
static inline int y2_fm_khz_valid(unsigned khz)
{
	return khz >= Y2_FM_KHZ_MIN && khz <= Y2_FM_KHZ_MAX && !(khz % Y2_FM_KHZ_STEP);
}
static inline unsigned y2_fm_unit(unsigned khz) { return khz / 10; }
static inline unsigned y2_fm_channel_reg(unsigned unit) { return (unit - 6400) * 2 / 10; }

/* mt6627_chan_para_map: one byte per 50 kHz from 65.00 MHz (861 entries). */
static const unsigned char y2_fm_chan_para_map[861] = {
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 8, 0, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0,
	1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0,
	0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
};
static inline unsigned y2_fm_chan_para(unsigned unit)
{
	unsigned pos;
	if (unit < 6500) return 0;
	pos = (unit - 6500) / 5;
	return y2_fm_chan_para_map[pos < 861 ? pos : 860];
}
/* mt6627_TDD_list/Mask: two bits per 50 kHz channel select the TDD solution. */
static const unsigned short y2_fm_tdd_list[216] = {
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0101,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0101, 0x0101, 0x0101, 0x0101, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0100, 0x0101, 0x0101, 0x0101,
	0x0101, 0x0101, 0x0101, 0x0101, 0x0001, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
	0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0100, 0x0101, 0x0101, 0x0101, 0x0101, 0x0101, 0x0001,
};
static inline int y2_fm_tdd(unsigned unit)
{
	static const unsigned short mask[4] = {0x0001, 0x0010, 0x0100, 0x1000};
	unsigned i = (unit - 6500) / 5;
	return i / 4 < 216 && (y2_fm_tdd_list[i / 4] & mask[i % 4]);
}
/* Desense list (scan only): a weak hit on these channels is a desense spur. */
static inline int y2_fm_desense_channel(unsigned unit)
{
	static const unsigned short list[12] = {
		6910, 7680, 7800, 9210, 9220, 9230, 9600, 9980, 9990, 10400, 10750, 10760,
	};
	for (unsigned i = 0; i < 12; i++) if (list[i] == unit) return 1;
	return 0;
}
static const unsigned short y2_fm_volume[16] = {
	0x0000, 0x0519, 0x066a, 0x0814, 0x0a2b, 0x0ccd, 0x101d, 0x1449,
	0x198a, 0x2027, 0x287a, 0x32f5, 0x4027, 0x50c3, 0x65ad, 0x7fff,
};

/* Receive thresholds: the stock kernel defaults, identical to the stock
 * /etc/fmr/mt6627_fm_cust.cfg, so the cfg file is not needed. */
#define Y2_FM_RSSI_TH (-296)
#define Y2_FM_DESENSE_RSSI_TH (-240)
#define Y2_FM_PAMD_TH (-12)
#define Y2_FM_MR_TH (-67)
#define Y2_FM_ATDC_TH 3496
#define Y2_FM_PRX_TH 64
#define Y2_FM_SMG_TH 16421

/* Register value -> dBm, stereo bit (mt6627_GetCurRSSI/GetMonoStereo). */
static inline int y2_fm_rssi_dbm(unsigned reg)
{
	int raw = reg & 0x3ff;
	return raw > 511 ? ((raw - 1024) * 6) >> 4 : (raw * 6) >> 4;
}
static inline int y2_fm_stereo(unsigned reg) { return !!(reg & 0x1000); }

/* Soft-mute tune result: cqi[0] size, then {ch, rssi, pamd, pr, fpamd, mr,
 * atdc, prx, atdev, smg, drssi} as 16-bit little-endian words. */
struct y2_fm_cqi { int rssi, valid; };
static inline unsigned y2_fm_cqi_word(const unsigned char *cqi, unsigned index)
{
	return cqi[2 + 2 * index] | (cqi[3 + 2 * index] << 8);
}
static inline struct y2_fm_cqi y2_fm_cqi_decide(const unsigned char *cqi)
{
	struct y2_fm_cqi r;
	int rssi = y2_fm_cqi_word(cqi, 1) & 0x3ff, pamd = y2_fm_cqi_word(cqi, 2) & 0x1ff;
	int mr = y2_fm_cqi_word(cqi, 5) & 0x1ff, atdc = y2_fm_cqi_word(cqi, 6);
	unsigned prx = y2_fm_cqi_word(cqi, 7) & 0xff, atdev = y2_fm_cqi_word(cqi, 8);
	unsigned smg = y2_fm_cqi_word(cqi, 9);
	rssi = rssi >= 512 ? rssi - 1024 : rssi;
	pamd = pamd >= 256 ? pamd - 512 : pamd;
	mr = mr >= 256 ? mr - 512 : mr;
	atdc = atdc >= 32768 ? 65536 - atdc : atdc;
	r.rssi = rssi;
	r.valid = rssi >= Y2_FM_RSSI_TH && pamd <= Y2_FM_PAMD_TH && atdc <= Y2_FM_ATDC_TH &&
		  mr >= Y2_FM_MR_TH && prx >= Y2_FM_PRX_TH && atdev >= (unsigned)atdc &&
		  smg >= Y2_FM_SMG_TH;
	return r;
}
/* mt6627_desense_check: 1 = reject a channel found during a scan. */
static inline int y2_fm_desense(unsigned unit, int raw_rssi)
{
	return y2_fm_desense_channel(unit) && raw_rssi < Y2_FM_DESENSE_RSSI_TH;
}

/* One received event: 04 op len_lo len_hi payload. Returns the bytes used
 * (0 = malformed/short), the event bits it raises and its payload. */
struct y2_fm_event { unsigned op, bits, size; const unsigned char *payload; };
static inline unsigned y2_fm_event_next(const unsigned char *p, unsigned size, struct y2_fm_event *e)
{
	unsigned len;
	if (size < 4 || p[0] != 0x04) return 0;
	len = p[2] | (p[3] << 8);
	if (len > size - 4) return 0;
	e->op = p[1]; e->size = len; e->payload = p + 4; e->bits = 0;
	if (!len) e->bits = e->op == Y2_FM_OP_CSPI_WRITE ? Y2_FM_EV_CSPI_WRITE :
			    e->op < 32 ? Y2_FM_EV(e->op) : 0;
	else if (e->op == Y2_FM_OP_TUNE) e->bits = len == 1 && p[4] == 1 ? Y2_FM_EV_TUNE_DONE : 0;
	else if (e->op == Y2_FM_OP_SOFT_MUTE_TUNE) e->bits = len >= 2 ? Y2_FM_EV(e->op) : 0;
	else if (e->op == Y2_FM_OP_CSPI_READ) e->bits = Y2_FM_EV_CSPI_READ;
	else if (e->op < 32) e->bits = Y2_FM_EV(e->op);
	return len + 4;
}
#endif
