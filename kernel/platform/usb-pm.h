/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_PM_H
#define Y2_USB_PM_H
#include <linux/kconfig.h>
struct musb;
#if IS_ENABLED(CONFIG_Y2_PLATFORM)
int y2_musb_system_quiesce(struct musb *musb);
void y2_musb_system_saved(struct musb *musb);
void y2_musb_before_restore(struct musb *musb);
#else
static inline int y2_musb_system_quiesce(struct musb *musb) { return 0; }
static inline void y2_musb_system_saved(struct musb *musb) { }
static inline void y2_musb_before_restore(struct musb *musb) { }
#endif
#endif
