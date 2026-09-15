/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_MD_RUNTIME_H
#define Y2_MD_RUNTIME_H
#include "protocol.h"
#define Y2_MD_ROM_BASE 0xbe000000U
#define Y2_MD_ROM_SIZE 0x1600000U
#define Y2_MD_SMEM_BASE 0xbf600000U
#define Y2_MD_SMEM_SIZE 0x1c4000U
#define Y2_MD_VIEW 0x41600000U
#define Y2_MD_FS_OFFSET 0xc0000U
#define Y2_MD_MAGIC 0x46494343U
/* Reconstructed from this product's stock ccci_alloc_smem and
 * ccci_send_run_time_data; no captured shared memory or private fixtures.
 * All addresses are modem-view addresses in the already excluded region.
 * No cellular/TTY/network consumer is exposed to Linux. */
static inline void y2_md_runtime(unsigned char *r, unsigned char *misc, int rtc_xtal)
{
	unsigned i;
	for (i = 0; i < 280; i++) r[i] = 0;
	for (i = 0; i < 288; i++) misc[i] = 0;
	y2_conn_put32(r, Y2_MD_MAGIC);
	y2_conn_put32(r + 4, 0x3536544d); /* MT6582E1 platform ABI */
	y2_conn_put32(r + 8, 0x31453238);
	y2_conn_put32(r + 12, 0x20121001);
	y2_conn_put32(r + 0x114, Y2_MD_MAGIC);
	/* Shared log, PCM, FS, RPC, exception, network, system and misc regions. */
	static const unsigned pairs[][3] = {
		{0x20,0x9000,0xb5000}, {0x28,0x1000,0x8000},
		{0x74,0xc0000,0x14014}, {0x7c,0xbe000,0x1008},
		{0x8c,0x118,0x800}, {0x9c,0x105270,0x2018},
		{0xa4,0x107288,0x4b000}, {0xac,0x152288,0x6fea0},
		{0xf8,0x918,12}, {0x100,0x105090,240}, {0x108,0x924,1024},
	};
	for (i = 0; i < sizeof(pairs) / sizeof(pairs[0]); i++) {
		y2_conn_put32(r + pairs[i][0], Y2_MD_VIEW + pairs[i][1]);
		y2_conn_put32(r + pairs[i][0] + 4, pairs[i][2]);
	}
	y2_conn_put32(r + 0x30, 8);
	for (i = 0; i < 6; i++) {
		y2_conn_put32(r + 0x34 + 4*i, Y2_MD_VIEW + 0xd5000 + i*0x8018);
		y2_conn_put32(r + 0x54 + 4*i, 0x8018);
	}
	y2_conn_put32(r + 0xb4, 3);
	for (i = 0; i < 3; i++) {
		unsigned base = Y2_MD_VIEW + 0x1c2128 + i*0xa20;
		y2_conn_put32(r + 0xd8 + 4*i, base);
		y2_conn_put32(r + 0xe8 + 4*i, 0x810);
		y2_conn_put32(r + 0xb8 + 4*i, base + 0x810);
		y2_conn_put32(r + 0xc8 + 4*i, 0x210);
	}
	/* Stock config_misc_info: remapped ROM base and read-only RTC_SPAR0[6]
	 * crystal-presence result. Do not guess or modify RTC spare fields. */
	y2_conn_put32(misc, 0x46494343);
	y2_conn_put32(misc + 4, rtc_xtal ? 6 : 10);
	y2_conn_put32(misc + 16, Y2_MD_ROM_BASE);
	y2_conn_put32(misc + 284, 0x46494343);
}
#endif
