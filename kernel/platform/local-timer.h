/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_LOCAL_TIMER_H
#define Y2_LOCAL_TIMER_H
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kconfig.h>
struct device_node;

#if IS_ENABLED(CONFIG_Y2_POWER)
struct device_node;
bool __init y2_local_timer_resource(struct device_node *node);
void __init y2_local_timer_prepare(void __iomem *gpt, unsigned long rate);
bool __init y2_local_timer_broadcast_prepare(void __iomem *gpt);
bool y2_local_timer_ready(void);
bool y2_local_events_ready(void);
void y2_local_timer_registration(int result);
int y2_local_timer_cpu_init(void);
#else
static inline bool y2_local_timer_resource(struct device_node *node) { return true; }
static inline void y2_local_timer_prepare(void __iomem *gpt, unsigned long rate) {}
static inline bool y2_local_timer_broadcast_prepare(void __iomem *gpt) { return false; }
static inline bool y2_local_timer_ready(void) { return false; }
static inline bool y2_local_events_ready(void) { return false; }
static inline void y2_local_timer_registration(int result) {}
static inline int y2_local_timer_cpu_init(void) { return 0; }
#endif
#endif
