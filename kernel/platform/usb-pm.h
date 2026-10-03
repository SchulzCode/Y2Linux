/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_PM_H
#define Y2_USB_PM_H
#include <linux/kconfig.h>
#include <linux/types.h>
struct musb;
#if IS_ENABLED(CONFIG_Y2_PLATFORM)
int y2_musb_system_quiesce(struct musb *musb);
void y2_musb_system_saved(struct musb *musb);
void y2_musb_before_restore(struct musb *musb);
void y2_musb_after_restore(struct musb *musb);
int y2_musb_runtime_gate(struct musb *musb);
int y2_musb_runtime_clock(struct musb *musb);
bool y2_usb_idle_ok(void);
#else
static inline int y2_musb_system_quiesce(struct musb *musb) { return 0; }
static inline void y2_musb_system_saved(struct musb *musb) { }
static inline void y2_musb_before_restore(struct musb *musb) { }
static inline void y2_musb_after_restore(struct musb *musb) { }
static inline int y2_musb_runtime_gate(struct musb *musb) { return 0; }
static inline int y2_musb_runtime_clock(struct musb *musb) { return 0; }
static inline bool y2_usb_idle_ok(void) { return true; }
#endif
#endif
