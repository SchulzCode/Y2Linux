/* SPDX-License-Identifier: GPL-2.0-only */
/* Host adapters only; parser bodies are the locked upstream Linux source. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <assert.h>
typedef uint64_t sector_t; typedef int16_t __s16; typedef uint32_t __u32; typedef uint16_t __u16; typedef uint8_t __u8;
typedef uint32_t u32; typedef uint8_t u8;
typedef uint16_t __le16; typedef uint32_t __le32;
typedef int Sector;
#define CONFIG_EFI_PARTITION 1
#define EFI_PMBR_OSTYPE_EFI_GPT 0xee
#define PAGE_SIZE 4096
#define ADDPART_FLAG_RAID 1
#define min(a,b) ((a)<(b)?(a):(b))
#define max(a,b) ((a)>(b)?(a):(b))
struct partition_meta_info {char uuid[40],volname[64];};
struct part {sector_t from,size;int flags;bool has_info;struct partition_meta_info info;};
struct disk {int queue;};
struct parsed_partitions {struct disk *disk;struct part parts[256];int next,limit;char pp_buf[4096];};
struct fat_boot_sector {char prefix[14];uint16_t reserved;uint8_t fats;char gap[4];uint8_t media;} __attribute__((packed));
static unsigned char sectors[3][512];
static unsigned char *read_part_sector(struct parsed_partitions *s, sector_t n, Sector *b) {(void)s;(void)b;return n==0?sectors[0]:n==1024?sectors[1]:n==145408?sectors[2]:NULL;}
static void put_dev_sector(Sector b) {(void)b;}
static unsigned queue_logical_block_size(int q) {(void)q;return 512;}
static u32 get_unaligned_le32(const void *p) {const uint8_t *b=p;return b[0]|b[1]<<8|b[2]<<16|((u32)b[3]<<24);}
#define le32_to_cpup get_unaligned_le32
static int fat_valid_media(unsigned m) {return m>=0xf8||m==0xf0;}
#define strlcat y2_strlcat
static void strlcat(char *d,const char *s,size_t n) {strncat(d,s,n-strlen(d)-1);}
static void put_partition(struct parsed_partitions *s,int slot,sector_t f,sector_t n) {s->parts[slot].from=f;s->parts[slot].size=n;}
