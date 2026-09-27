/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_LOCAL_TIMER_H
#define Y2_LOCAL_TIMER_H
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kconfig.h>

#if IS_ENABLED(CONFIG_Y2_POWER)
void __init y2_local_timer_prepare(void __iomem *gpt, unsigned long rate);
bool y2_local_timer_ready(void);
int y2_local_timer_cpu_init(void);
#else
static inline void y2_local_timer_prepare(void __iomem *gpt, unsigned long rate) {}
static inline bool y2_local_timer_ready(void) { return false; }
static inline int y2_local_timer_cpu_init(void) { return 0; }
#endif
#endif
