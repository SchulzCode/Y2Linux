/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_PWRAP_READINESS_POLICY_H
#define Y2_PWRAP_READINESS_POLICY_H
/* MT6582 PMIC_WRAP MUX_SEL(+0x00), WRAP_EN(+0x04), HIPRIO_ARB_EN(+0x50).
 * The pinned MT6582 BSP defines seven arbiter channels: MDINF bit0, WACS0..2
 * bits1..3, DVFSINF bit4 (SPM voltage slots), STAUPD bit5, GPSINF bit6.
 * The retained Y2 preloader pwrap_init stores 0x1ff (file offset 0xc954);
 * the unimplemented bits 7/8 read back as zero, and M2-PWRAP-01 physically
 * photographed ARB=0x7f. Readiness therefore compares the implemented field
 * against the complete preloader configuration. It never accepts a disabled
 * DVFS/WACS2 channel, manual mux, disabled wrapper or unknown readback bits. */
#define Y2_PWRAP_ARB_IMPLEMENTED 0x7fU
#define Y2_PWRAP_ARB_WACS2 (1U << 3)
#define Y2_PWRAP_ARB_DVFSINF (1U << 4)
static inline const char *y2_pwrap_dvfs_unready(unsigned mux, unsigned wrap,
						unsigned arb)
{
	if (mux != 0)
		return "mux_not_wrapper";
	if (wrap != 1)
		return "wrapper_disabled";
	if (arb & ~Y2_PWRAP_ARB_IMPLEMENTED)
		return "arbiter_unknown_bits";
	if (!(arb & Y2_PWRAP_ARB_DVFSINF))
		return "arbiter_dvfs_channel_disabled";
	if (!(arb & Y2_PWRAP_ARB_WACS2))
		return "arbiter_wacs2_disabled";
	if (arb != Y2_PWRAP_ARB_IMPLEMENTED)
		return "arbiter_not_preloader_complete";
	return 0;
}
#endif
