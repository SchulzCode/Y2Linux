// SPDX-License-Identifier: GPL-2.0
/*
 * Adapted from Chris Hendrickson, artificery-dev/linux 53fb57bb99c9.
 * MediaTek MT6582 MultiMedia (DISP) clock-gate driver.
 *
 * The MMSYS block (0x14000000) holds the DISP0/DISP1 clock gates for the
 * display data path (OVL/RDMA/COLOR/MUTEX/DSI). Register offsets and the gate
 * bit layout are identical to mt2701-mm (verified against the vendor
 * mt_clkmgr.c CG_DISP0/CG_DISP1 groups). This driver is spawned by name from
 * the mtk-mmsys parent device (see mtk-mmsys.c .clk_driver), not from its own
 * DT node.
 */
#include <linux/clk-provider.h>
#include <linux/platform_device.h>

#include "/src/drivers/clk/mediatek/clk-gate.h"
#include "/src/drivers/clk/mediatek/clk-mtk.h"

#include "mt6582-clk.h"

static const struct mtk_gate_regs disp0_cg_regs = {
    .set_ofs = 0x0104,
    .clr_ofs = 0x0108,
    .sta_ofs = 0x0100,
};

static const struct mtk_gate_regs disp1_cg_regs = {
    .set_ofs = 0x0114,
    .clr_ofs = 0x0118,
    .sta_ofs = 0x0110,
};

#define GATE_DISP0(_id, _name, _parent, _shift)                                                    \
	GATE_MTK_FLAGS(_id, _name, _parent, &disp0_cg_regs, _shift, &mtk_clk_gate_ops_setclr,      \
		       CLK_IGNORE_UNUSED)

/* Shared display bus gates stay on during this bounded handoff baseline;
 * LK consumers are not all represented by Linux clock references yet. */
#define GATE_DISP0_SHARED(_id, _name, _parent, _shift)                                             \
	GATE_MTK_FLAGS(_id, _name, _parent, &disp0_cg_regs, _shift, &mtk_clk_gate_ops_setclr,      \
		       CLK_IGNORE_UNUSED | CLK_IS_CRITICAL)

#define GATE_DISP1(_id, _name, _parent, _shift)                                                    \
	GATE_MTK_FLAGS(_id, _name, _parent, &disp1_cg_regs, _shift, &mtk_clk_gate_ops_setclr,      \
		       CLK_IGNORE_UNUSED)

static const struct mtk_gate mm_clks[] = {
    /* DISP0 */
    GATE_DISP0_SHARED(CLK_MM_SMI_COMMON, "mm_smi_common", "y2-mm-inherited-rate-unresolved", 0),
    GATE_DISP0_SHARED(CLK_MM_SMI_LARB0, "mm_smi_larb0", "y2-mm-inherited-rate-unresolved", 1),
    GATE_DISP0(CLK_MM_CMDQ, "mm_cmdq", "y2-mm-inherited-rate-unresolved", 2),
    GATE_DISP0_SHARED(CLK_MM_MUTEX, "mm_mutex", "y2-mm-inherited-rate-unresolved", 3),
    GATE_DISP0(CLK_MM_DISP_COLOR, "mm_disp_color", "y2-mm-inherited-rate-unresolved", 4),
    GATE_DISP0(CLK_MM_DISP_BLS, "mm_disp_bls", "y2-pwm-inherited-rate-unresolved", 5),
    GATE_DISP0(CLK_MM_DISP_WDMA, "mm_disp_wdma", "y2-mm-inherited-rate-unresolved", 6),
    GATE_DISP0(CLK_MM_DISP_RDMA, "mm_disp_rdma", "y2-mm-inherited-rate-unresolved", 7),
    GATE_DISP0(CLK_MM_DISP_OVL, "mm_disp_ovl", "y2-mm-inherited-rate-unresolved", 8),
    GATE_DISP0(CLK_MM_MDP_TDSHP, "mm_mdp_tdshp", "y2-mm-inherited-rate-unresolved", 9),
    GATE_DISP0(CLK_MM_MDP_WROT, "mm_mdp_wrot", "y2-mm-inherited-rate-unresolved", 10),
    GATE_DISP0(CLK_MM_MDP_WDMA, "mm_mdp_wdma", "y2-mm-inherited-rate-unresolved", 11),
    GATE_DISP0(CLK_MM_MDP_RSZ1, "mm_mdp_rsz1", "y2-mm-inherited-rate-unresolved", 12),
    GATE_DISP0(CLK_MM_MDP_RSZ0, "mm_mdp_rsz0", "y2-mm-inherited-rate-unresolved", 13),
    GATE_DISP0(CLK_MM_MDP_RDMA, "mm_mdp_rdma", "y2-mm-inherited-rate-unresolved", 14),
    GATE_DISP0(CLK_MM_MDP_BLS_26M, "mm_mdp_bls_26m", "y2-pwm-inherited-rate-unresolved", 15),
    GATE_DISP0(CLK_MM_CAM_MDP, "mm_cam_mdp", "y2-mm-inherited-rate-unresolved", 16),
    GATE_DISP0(CLK_MM_FAKE_ENG, "mm_fake_eng", "y2-mm-inherited-rate-unresolved", 17),
    GATE_DISP0(CLK_MM_MUTEX_32K, "mm_mutex_32k", "y2-rtc32k", 18),
    /* DISP1 */
    GATE_DISP1(CLK_MM_DSI_ENGINE, "mm_dsi_engine", "y2-mm-inherited-rate-unresolved", 0),
    GATE_DISP1(CLK_MM_DSI_DIGITAL, "mm_dsi_digital", "clk26m", 1),
    GATE_DISP1(CLK_MM_DPI_DIGITAL_LANE, "mm_dpi_digital_lane", "y2-mm-inherited-rate-unresolved",
	       2),
    GATE_DISP1(CLK_MM_DPI_ENGINE, "mm_dpi_engine", "y2-mm-inherited-rate-unresolved", 3),
};

static const struct mtk_clk_desc mm_desc = {
    .clks = mm_clks,
    .num_clks = ARRAY_SIZE(mm_clks),
};

static const struct platform_device_id clk_mt6582_mm_id_table[] = {
    {.name = "clk-mt6582-mm", .driver_data = (kernel_ulong_t)&mm_desc}, {/* sentinel */}};
MODULE_DEVICE_TABLE(platform, clk_mt6582_mm_id_table);

static struct platform_driver clk_mt6582_mm_drv = {
    .probe = mtk_clk_pdev_probe,
    .remove = mtk_clk_pdev_remove,
    .driver =
	{
	    .name = "clk-mt6582-mm",
	},
    .id_table = clk_mt6582_mm_id_table,
};
module_platform_driver(clk_mt6582_mm_drv);

MODULE_DESCRIPTION("MediaTek MT6582 MultiMedia ddp clocks driver");
MODULE_LICENSE("GPL");
