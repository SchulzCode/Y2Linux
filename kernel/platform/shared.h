/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PLATFORM_SHARED_H
#define Y2_PLATFORM_SHARED_H
struct y2_pwrap_snapshot;
int y2_msdc_crystal(unsigned id);
int y2_ccf_usb_read(unsigned address, unsigned *value);
int y2_pmic_snapshot(struct y2_pwrap_snapshot *snapshot);
#endif
