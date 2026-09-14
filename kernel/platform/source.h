/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SOURCE_H
#define Y2_SOURCE_H
#include <linux/types.h>
/* USB owns the PHY and serializes classification against gadget lifecycle.
 * A successful begin holds that ownership until end, including on IO error. */
int y2_usb_bc11_begin(void);
int y2_usb_bc11_end(bool data_source);
void y2_usb_source_invalidate(void);
int y2_usb_charge_allocation(void);
#endif
