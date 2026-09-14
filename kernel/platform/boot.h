/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_BOOT_H
#define Y2_BOOT_H
#include <linux/types.h>
#include <linux/notifier.h>
bool y2_normal_boot_enabled(void);
int y2_boot_register_notifier(struct notifier_block *nb);
void y2_boot_unregister_notifier(struct notifier_block *nb);
#endif
