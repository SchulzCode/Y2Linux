// SPDX-License-Identifier: GPL-2.0-only
/* One bounded read from reserved loader RAM, no MMIO or writes. */
#include <linux/module.h>
#include <linux/io.h>
static int __init snapshot_init(void)
{
 const u32 *p = phys_to_virt(0x80000100);
 unsigned off = 0, limit = 0x3f00 / 4;
 while (off + 2 <= limit) {
  u32 words = p[off], tag = p[off + 1];
  if (!words) break;
  if (words < 2 || words > limit - off) return -EINVAL;
  if (tag == 0x41000804) {
   if (words != 25 || p[off + 24] != 22) return -EPROTO;
   pr_info("Y2GPU_READ_ONLY: ATAG_DEVINFO_INDEX3_BIT19=%u words=%u count=%u\n",
           !!(p[off + 5] & BIT(19)), words, p[off + 24]);
   return -EAGAIN;
  }
  off += words;
 }
 pr_info("Y2GPU_READ_ONLY: ATAG_DEVINFO absent\n");
 return -ENOENT;
}
module_init(snapshot_init);
MODULE_LICENSE("GPL");
