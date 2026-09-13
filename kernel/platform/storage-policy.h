/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_STORAGE_POLICY_H
#define Y2_STORAGE_POLICY_H
#include "storage-layout.h"
/* Policy for the real Y2 internal MMC controller, in stock logical 512-byte sectors.
 * A companion command never grants access independently of its data request.
 * PIO/DMA and normal/rescue userspace all share this boundary. */
#define Y2_MMC_WRITE (1U << 8)
#define Y2_MMC_READ (1U << 9)
struct y2_emmc_request {
    unsigned opcode, arg, direction, blocks, blksz;
    unsigned has_sbc, sbc_opcode, sbc_arg;
    unsigned has_stop, stop_opcode, stop_arg;
    unsigned user_area, part_config, identified;
};
static inline int y2_emmc_write_range(unsigned sector, unsigned blocks)
{
    unsigned end;
    if (!blocks || blocks > 0xffffffffU - sector) return 0;
    end = sector + blocks;
    return (sector >= 166912 && end <= 1846272) ||
           (sector >= 2104320 && end <= 3742720);
}
static inline int y2_emmc_request_allowed(const struct y2_emmc_request *r)
{
    unsigned flags, index, value;
    int write = r->direction == Y2_MMC_WRITE;
    int read = r->direction == Y2_MMC_READ;
    if (r->direction && !write && !read) return 0;
    if (r->has_stop && (r->stop_opcode != 12 || r->stop_arg)) return 0;
    if (r->has_sbc) {
        flags = write ? 0xa0000000U : 0; /* reliable write + metadata tag */
        if ((!read && !write) || !r->user_area || r->blksz != 512 ||
            r->opcode != (write ? 25U : 18U) || r->sbc_opcode != 23 ||
            !r->blocks || r->blocks > 65535 ||
            (r->sbc_arg & ~flags) != r->blocks) return 0;
    }
    if (write || (read && (r->opcode == 17 || r->opcode == 18))) {
        if (!r->user_area || r->blksz != 512 || !r->blocks ||
            r->blocks > Y2_EMMC_DISK_SECTORS ||
            r->arg > Y2_EMMC_DISK_SECTORS - r->blocks) return 0;
        if (write && !y2_emmc_write_range(r->arg, r->blocks)) return 0;
        return r->opcode == (write ? 25U : 18U) ||
               (r->blocks == 1 && r->opcode == (write ? 24U : 17U));
    }
    /* EXT_CSD is a standard read during identification, before host->card exists. */
    if (read) return r->opcode == 8 && !r->arg &&
                     r->blksz == 512 && r->blocks == 1 && !r->has_stop;
    if (r->blocks || r->blksz || r->has_sbc || r->has_stop) return 0;
    if (r->opcode == 6) {
        if ((r->arg & 0xff0000ffU) != 0x03000001U) return 0;
        index = (r->arg >> 16) & 255; value = (r->arg >> 8) & 255;
        switch (index) {
        case 32: return value == 1;                 /* FLUSH_CACHE */
        case 33: return value <= 1;                 /* CACHE_CTRL */
        case 34: return value >= 1 && value <= 3;   /* power notification */
        case 161: return value <= 1;                /* HPI management */
        case 175: return value == 1;                /* ERASE_GROUP_DEF */
        case 179: return r->identified && value == (r->part_config & ~7U);
        case 183: return value == 0;                /* supported one-bit bus */
        case 185: return value == 0;                /* supported legacy timing */
        default: return 0;
        }
    }
    switch (r->opcode) {
    case 0: return r->arg == 0; /* not boot/FFU/reset-to-alternate-mode */
    case 1: case 2: case 3: case 7: case 9: case 10: case 12: case 13:
        return 1; /* identification, select, status, stop: no persistent writes */
    case 5: return r->user_area; /* standard sleep/awake, no storage write */
    case 16: return r->arg == 512;
    default: return 0; /* erase/sanitize, RPMB, boot, WP, lock, vendor/FFU */
    }
}
#endif
