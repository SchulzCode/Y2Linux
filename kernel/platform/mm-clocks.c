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
#include <linux/clk.h>
#include <linux/io.h>
#include <linux/of_address.h>
#include <linux/pm_domain.h>
#include <linux/pm_runtime.h>
#include <linux/reboot.h>
#include "clocks.h"
#include "spm.h"

#include "/src/drivers/clk/mediatek/clk-gate.h"
#include "/src/drivers/clk/mediatek/clk-mtk.h"

#include "mt6582-clk.h"
#include "dcm-policy.h"

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

/* LK handoff remains protected from the unused-clock sweep. After modeset,
 * mtk_mutex owns all three shared clocks through a balanced bulk lifecycle;
 * Lima has an independent SMI_COMMON reference. No permanent critical hold. */
#define GATE_DISP0_SHARED(_id, _name, _parent, _shift)                                             \
	GATE_MTK_FLAGS(_id, _name, _parent, &disp0_cg_regs, _shift, &mtk_clk_gate_ops_setclr,      \
		       CLK_IGNORE_UNUSED)

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
/* POWER-FINAL-16 G1: with the DIS power domain, every gate register access
 * goes through the clock core's provider runtime PM, which powers DIS first. */
static const struct mtk_clk_desc mm_desc_dis = {
    .clks = mm_clks,
    .num_clks = ARRAY_SIZE(mm_clks),
    .need_runtime_pm = true,
};
static const struct platform_device_id mm_dis_id = {
    .name = "clk-mt6582-mm", .driver_data = (kernel_ulong_t)&mm_desc_dis,
};
static struct device *mm_dev;

static void __iomem *mm_idle_base;
static struct clk *mm_idle_clocks[CLK_MM_NR_CLK];
static unsigned reclaimed, retained, reclaim_failures;
static bool bls_handoff;
static bool handoff_broken;
module_param(reclaimed, uint, 0400);
module_param(retained, uint, 0400);
module_param(reclaim_failures, uint, 0400);
module_param(bls_handoff, bool, 0400);

/* POWER-FINAL-16 G3: stock SMI/DIS DCM, re-applied at every CRTC enable
 * because DIS power-down resets MMSYS (dcm-policy.h). */
static bool dcm = true;
module_param(dcm, bool, 0644);
static unsigned dcm_applied;
module_param(dcm_applied, uint, 0400);
static unsigned y2_mm_dcm_read(void *context, unsigned phys)
{
	return readl(mm_idle_base + (phys - 0x14000000));
}
static void y2_mm_dcm_write(void *context, unsigned phys, unsigned value)
{
	writel(value, mm_idle_base + (phys - 0x14000000));
	dsb(sy);
}
void y2_mm_dcm_apply(void)
{
	static const struct y2_dcm_io io = { NULL, y2_mm_dcm_read, y2_mm_dcm_write };
	if (!READ_ONCE(dcm) || !mm_idle_base || y2_spm_disp_status() != 1) return;
	y2_dcm_mm(&io);
	dcm_applied++;
}

int y2_mm_idle_blockers(unsigned *disp0, unsigned *disp1)
{
	unsigned long flags;
	int powered = y2_spm_disp_status();
	*disp0 = *disp1 = 0;
	if (powered < 0) return powered;
	if (!mm_idle_base) return -ENODEV;
	/* Never read an unpowered or bus-protected window (DIS genpd, 16 G1). */
	if (y2_spm_disp_access_begin(&flags)) {
		*disp0 = ~readl(mm_idle_base + 0x100) & 0x7ffff;
		*disp1 = ~readl(mm_idle_base + 0x110) & 0xf;
	}
	y2_spm_disp_access_end(flags);
	return 0;
}

/* Called by the real MT6582 CRTC/mutex owner after engines/mutex stop and
 * before its shared bus clocks are released. Unsupported loader engines get
 * a normal CCF acquire/release only when their exact BSP idle operands pass.
 * No reset, DMA abort, interrupt clear, gate-mask override or active-engine
 * forced shutdown is used. CMDQ has seven threads, not a newer SoC's layout. */
static bool y2_mm_unused_quiet(unsigned id)
{
	void __iomem *r = mm_idle_base;
	unsigned i;
	if (readl(r + 0x10450)) return false; /* SMI_LARB0 GREQ */
	switch (id) {
	case CLK_MM_CMDQ:
		for (i = 0; i < 7; i++)
			if (readl(r + 0xf104 + 0x80 * i) & 1) return false;
		return true;
	case CLK_MM_DISP_BLS: case CLK_MM_MDP_BLS_26M:
		return !(readl(r + 0xa000) & 0x10001);
	case CLK_MM_DISP_WDMA: return !(readl(r + 0x9008) & 1);
	case CLK_MM_MDP_WDMA: return !(readl(r + 0x4008) & 1);
	/* Exact MT6582 ddp_cmdq_debug.c: ROT_ROT_EN=0x7c,
	 * ROT_RST_STAT=0x14. A completed reset alone is not an idle proof. */
	case CLK_MM_MDP_WROT:
		return !readl(r + 0x507c) && !(readl(r + 0x5014) & 1);
	case CLK_MM_MDP_TDSHP: return !readl(r + 0x6100);
	case CLK_MM_MDP_RSZ0: return !readl(r + 0x2000);
	case CLK_MM_MDP_RSZ1: return !readl(r + 0x3000);
	case CLK_MM_MDP_RDMA: return (readl(r + 0x1408) & 0x7ff00) == 0x100;
	case CLK_MM_CAM_MDP: case CLK_MM_FAKE_ENG:
		/* MT6582 FAKE_ENG_BASE=0x15002000, CAM=ISP. No unpowered read. */
		return y2_spm_isp_status() == 0;
	case CLK_MM_DPI_DIGITAL_LANE: case CLK_MM_DPI_ENGINE:
		/* A partial loader handoff does not justify reading an unclocked
		 * partner's operands. Retain the remaining gate conservatively. */
		if (!mm_idle_clocks[CLK_MM_DPI_DIGITAL_LANE] ||
		    !mm_idle_clocks[CLK_MM_DPI_ENGINE] ||
		    !__clk_is_enabled(mm_idle_clocks[CLK_MM_DPI_DIGITAL_LANE]) ||
		    !__clk_is_enabled(mm_idle_clocks[CLK_MM_DPI_ENGINE])) return false;
		return !(readl(r + 0xd000) & 1) && !(readl(r + 0xd040) & 1);
	default: return false; /* Linux-owned live pipeline is never reclaimed */
	}
}
static void y2_mm_reclaim_powered(void);
void y2_mm_reclaim_unused(void)
{
	/* Hold DIS across every direct register read below. */
	if (mm_dev && pm_runtime_resume_and_get(mm_dev) < 0) return;
	y2_mm_reclaim_powered();
	if (mm_dev) pm_runtime_put(mm_dev);
}
static void y2_mm_reclaim_powered(void)
{
	unsigned id;
	bool quiet[CLK_MM_NR_CLK] = { false };
	if (!mm_idle_base || handoff_broken || y2_spm_disp_status() != 1) return;
	/* Linux's Y2 OVL -> RDMA -> COLOR -> DSI route bypasses BLS. Only
	 * after its CRTC/mutex has stopped may this owner retire LK's BLS/PWM
	 * enables. BLS has no DMA; MT6582 ddp_bls.c defines EN bits 0/16. The
	 * Y2 backlight is independently owned by the PMIC backlight driver. */
	if (!bls_handoff && !readl(mm_idle_base + 0x10450)) {
		struct clk *bls = mm_idle_clocks[CLK_MM_DISP_BLS];
		struct clk *pwm = mm_idle_clocks[CLK_MM_MDP_BLS_26M];
		if (bls && pwm && !clk_prepare_enable(bls)) {
			if (!clk_prepare_enable(pwm)) {
				unsigned value = readl(mm_idle_base + 0xa000);
				writel(value & ~0x10001U, mm_idle_base + 0xa000);
				bls_handoff = !(readl(mm_idle_base + 0xa000) & 0x10001);
				if (!bls_handoff) {
					reclaim_failures++; handoff_broken = true;
					return; /* retain both clocks on failed stop */
				}
				clk_disable_unprepare(pwm);
			} else {
				reclaim_failures++; handoff_broken = true;
				return; /* retain the acquired clock; never gate live BLS */
			}
			clk_disable_unprepare(bls);
		} else { reclaim_failures++; handoff_broken = true; return; }
	}
	/* Snapshot all operands before releasing any coupled engine clocks. */
	for (id = 0; id < CLK_MM_NR_CLK; id++) {
		struct clk *clock = mm_idle_clocks[id];
		if (!clock || !__clk_is_enabled(clock)) continue;
		if (id == CLK_MM_SMI_COMMON || id == CLK_MM_SMI_LARB0 ||
		    id == CLK_MM_MUTEX || id == CLK_MM_DISP_COLOR ||
		    id == CLK_MM_DISP_RDMA || id == CLK_MM_DISP_OVL ||
		    id == CLK_MM_MUTEX_32K || id == CLK_MM_DSI_ENGINE ||
		    id == CLK_MM_DSI_DIGITAL) continue;
		quiet[id] = y2_mm_unused_quiet(id);
		if (!quiet[id]) retained++;
	}
	for (id = 0; id < CLK_MM_NR_CLK; id++) {
		struct clk *clock = mm_idle_clocks[id];
		if (!quiet[id]) continue;
		if (clk_prepare_enable(clock)) { reclaim_failures++; continue; }
		clk_disable_unprepare(clock);
		if (__clk_is_enabled(clock)) reclaim_failures++;
		else reclaimed++;
	}
}
/* POWER-FINAL-43: LK's display is still fetching through SMI common and
 * larb0 when Linux boots, but no Linux driver owns them yet. Lima also uses
 * SMI common and goes runtime idle 200 ms after its probe; if the display
 * handover is later than that (restart boots, and power-on from charging
 * with USB plugged in: about 0.9 s), the last reference drops, CCF closes the
 * SMI common gate under LK's running fetch and larb0 wedges: no Linux frame
 * (archives 137-145). Hold both gates for LK's display from probe until the
 * display mutex has taken its own references. */
static bool loader_hold;
void y2_mm_release_loader_hold(void)
{
	if (!xchg(&loader_hold, false)) return;
	clk_disable_unprepare(mm_idle_clocks[CLK_MM_SMI_LARB0]);
	clk_disable_unprepare(mm_idle_clocks[CLK_MM_SMI_COMMON]);
}
EXPORT_SYMBOL_GPL(y2_mm_release_loader_hold);
static void y2_mm_take_loader_hold(struct device *dev)
{
	struct clk *smi = mm_idle_clocks[CLK_MM_SMI_COMMON], *larb = mm_idle_clocks[CLK_MM_SMI_LARB0];
	if (!smi || !larb) return;
	if (!__clk_is_enabled(smi)) {
		/* Before Lima or the display ran: the previous kernel's restart
		 * left the gate closed (see y2_mm_restart), and LK's first fetch
		 * through larb0 is stuck (2026-10-18, Beta 8.3 OTA). */
		if (mm_idle_base && y2_spm_disp_status() == 1)
			dev_warn(dev, "SMI common gated before Linux touched it: CG_CON0=%08x larb0 stat=%08x greq=%08x\n",
				 readl(mm_idle_base + 0x100), readl(mm_idle_base + 0x10000),
				 readl(mm_idle_base + 0x10450));
		return;
	}
	if (clk_prepare_enable(smi)) return;
	if (clk_prepare_enable(larb)) { clk_disable_unprepare(smi); return; }
	loader_hold = true;
	dev_info(dev, "holding SMI common/larb0 for the loader display until the display takes over\n");
}
/* POWER-FINAL-42, back in Beta 8.4: a software restart (watchdog reset)
 * keeps MMSYS_CG_CON0. At shutdown DRM disables the display and drops its
 * SMI references; with Lima idle the SMI common gate closes, and when the
 * restart comes before the DIS domain has powered down the closed gate
 * survives into LK, whose first fetch then wedges larb0 for good (40/41;
 * the 8.3 OTA restart on 2026-10-18: no hold taken at probe, "SMI gates 1
 * left closed" at handover, no frame). 43 removed this handler because its
 * own test restarts were dark for a second reason, Lima closing the gate
 * during the next boot, which y2_mm_take_loader_hold() now covers. As the
 * last step before the watchdog handler, after every driver has shut down,
 * reopen both gates so LK finds the cold-start state. Only with the DIS
 * domain powered (read from SPM, no lock: other CPUs are stopped); a
 * powered-down domain comes back with its gates open anyway. */
static int y2_mm_restart(struct notifier_block *nb, unsigned long mode, void *cmd)
{
	if (mm_idle_base && y2_spm_disp_status() == 1)
		writel(0x3, mm_idle_base + 0x108); /* MMSYS_CG_CLR0: SMI common, larb0 */
	return NOTIFY_DONE;
}
static struct notifier_block y2_mm_restart_nb = {
	.notifier_call = y2_mm_restart,
	.priority = 255, /* before the watchdog restart handler (128) */
};
static int y2_mm_probe(struct platform_device *pdev)
{
	struct clk_hw_onecell_data *data;
	struct resource resource;
	unsigned i;
	int ret;
	/* Shared read-only engine diagnostics; MMSYS/DRM retain each resource. */
	if (of_address_to_resource(pdev->dev.parent->of_node, 0, &resource)) return -EINVAL;
	/* The SPM owner registers DIS; without it, keep 15's always-on gates. */
	if (y2_spm_disp_status() == -EPROBE_DEFER) return -EPROBE_DEFER;
	if (y2_spm_dis_genpd()) {
		ret = pm_genpd_add_device(y2_spm_dis_genpd(), &pdev->dev);
		if (ret) return ret;
		pdev->id_entry = &mm_dis_id;
		mm_dev = &pdev->dev;
	}
	mm_idle_base = devm_ioremap(&pdev->dev, resource.start, 0x12000);
	if (!mm_idle_base) ret = -ENOMEM;
	else ret = mtk_clk_pdev_probe(pdev);

	if (ret) {
		mm_idle_base = NULL;
		if (mm_dev) { pm_genpd_remove_device(&pdev->dev); mm_dev = NULL; }
		return ret;
	}
	data = platform_get_drvdata(pdev);
	for (i = 0; i < CLK_MM_NR_CLK; i++) {
		mm_idle_clocks[i] = devm_clk_hw_get_clk(&pdev->dev, data->hws[i], "idle-handoff");
		if (IS_ERR(mm_idle_clocks[i])) {
			ret = PTR_ERR(mm_idle_clocks[i]); mm_idle_clocks[i] = NULL;
			dev_warn(&pdev->dev, "idle handoff clock%u unavailable: %d\n", i, ret);
		}
	}
	y2_mm_take_loader_hold(&pdev->dev);
	{
		static bool restart_registered;
		if (!restart_registered && !register_restart_handler(&y2_mm_restart_nb))
			restart_registered = true;
	}
	return 0;
}

static const struct platform_device_id clk_mt6582_mm_id_table[] = {
    {.name = "clk-mt6582-mm", .driver_data = (kernel_ulong_t)&mm_desc}, {/* sentinel */}};
MODULE_DEVICE_TABLE(platform, clk_mt6582_mm_id_table);

static struct platform_driver clk_mt6582_mm_drv = {
    .probe = y2_mm_probe,
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
