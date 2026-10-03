/* SPDX-License-Identifier: GPL-2.0-only */
#define Y2_CLK_ARMPLL 0
#define Y2_CLK_MAINPLL 1
#define Y2_CLK_UNIVPLL 2
#define Y2_CLK_MMPLL 3
#define Y2_CLK_MSDCPLL 4
#define Y2_CLK_AXI 5
#define Y2_CLK_I2C0 6
#define Y2_CLK_I2C1 7
#define Y2_CLK_APDMA 8
#define Y2_CLK_PWRAP 9
#define Y2_CLK_KP 10
#define Y2_CLK_MSDC0 11
#define Y2_CLK_MSDC1 12
#define Y2_CLK_MSDC0_SRC 13
#define Y2_CLK_MSDC1_SRC 14
#define Y2_CLK_AUDINTBUS 15
#define Y2_CLK_AUDIO 16
#define Y2_CLK_INFRA_AUDIO 17
#define Y2_CLK_CPU 18
#define Y2_CLK_THERM 19
#define Y2_CLK_AUXADC 20
#define Y2_CLK_EFUSE 21
#define Y2_CLK_CONNMCU 22
#define Y2_CLK_BTIF 23
#define Y2_CLK_MFG_SRC 24
#define Y2_CLK_MSDC2 25
#define Y2_CLK_I2C2 26
#define Y2_CLK_UNUSED_START 27
#define Y2_CLK_USB0 44
#define Y2_CLK_NR 45
#ifndef __DTS__
int y2_ccf_slow_idle(void);
int y2_ccf_deep_idle_begin(void);
int y2_ccf_deep_idle_end(void);
int y2_ccf_deep_idle_blockers(unsigned *peri, unsigned *infra, unsigned *bus);
int y2_mm_idle_blockers(unsigned *disp0, unsigned *disp1);
void y2_mm_reclaim_unused(void);
int y2_ccf_usb_claim(void);
#endif
