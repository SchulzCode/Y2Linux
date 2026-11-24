# STP power saving (PSM) for Y2 connectivity: protocol and design

Research 2026-11-24 for the power plan, phase 7 ([POWER-PLAN-2026-11.md](../planning/POWER-PLAN-2026-11.md)); implemented in
`kernel/platform/connectivity/stp-psm.c`, off unless `stp_psm.enable=Y`. Paths are relative to
`donorSource/drivers/misc/mediatek-consys/` (`D/`) or to the repository root. `donorSource` is the vendor GPL tree,
already partly ported to mainline; points checked against the stock kernel `out/rea/fm-kernel.elf` are marked **[ELF]**.

---

## 1. The PSM protocol

### 1.1 Wire messages (WMT channel = STP channel 4, ordinary full-STP frames with seq/ack/CRC)

`D/common/core/wmt_core.c:173-180`

| Message | Direction | Bytes |
| --- | --- | --- |
| SLEEP_CMD | host → chip | `01 03 01 00 01` |
| SLEEP_EVT | chip → host | `02 03 02 00 00 01` |
| HOST_AWAKE_CMD | host → chip | `01 03 01 00 02` (exists, **not used**, see 1.3) |
| HOST_AWAKE_EVT | chip → host | `02 03 02 00 00 02` |
| WAKEUP_CMD | host → chip | `FF` (raw; inside `#if 0`, **not used on SOC**) |
| WAKEUP_EVT | chip → host | `02 03 02 00 00 03` |

All of them are WMT opcode 0x03 with a one-byte parameter, so they fit `y2_conn_wmt()`'s
reply check (`kernel/platform/connectivity/wmt.c:35-37`: size, opcode, status byte 0).

### 1.2 Sleep (host → chip)

`opfunc_pwr_sv()` SLEEP branch, `D/common/core/wmt_core.c:1245-1282`:
1. `wmt_core_tx(SLEEP_CMD)` (1249).
2. `wmt_core_rx()` for 6 bytes, timeout `WMT_LIB_RX_TIMEOUT` = 2000 ms (`D/common/core/include/wmt_lib.h:60`).
3. On timeout or short read: flush, dump, **firmware assert** (`WMT_CTRL_EVT_ERR_TRG_ASSERT`, reason 33, 1255-1265).
   On mismatch: flush and log only (1268-1278).
4. In **every** case (also on failure), `pwr_sv_done:` calls the PSM callback with the action
   (1374-1385) → PSM moves to INACT and gates the BTIF clocks even after a failed sleep.
   (We should not copy that.)

The SLEEP command bypasses the PSM gate in the STP send path because it is a "privileged command"
(`D/common/core/stp_core.c:591-631`, table `01 03 01 00 01` and `01 03 01 00 02`) or because WMT
traffic skips monitoring in quick mode (`stp_core.c:2728-2730`). STP ACKs are written straight to
BTIF, never through PSM (`stp_send_ack`, `stp_core.c:822-870`).

### 1.3 Wake-up (host → chip): a BTIF register pulse, then wait for WAKEUP_EVT

`opfunc_pwr_sv()` WAKEUP branch, `wmt_core.c:1284-1332`:
1. `wmt_core_ctrl(WMT_CTRL_SOC_WAKEUP_CONSYS)` (1288) → `wmt_ctrl_soc_wakeup_consys`
   (`D/common/core/wmt_ctrl.c:611`) → `mtk_wcn_stp_wakeup_consys` (`stp_core.c:3592`) →
   `mtk_wcn_consys_stp_btif_wakeup` (`D/common/linux/pri/stp_btif.c:203`) →
   `mtk_wcn_btif_wakeup_consys` (`D/btif/mtk_btif_exp.c:358`) → `btif_raise_wak_signal`
   (`D/btif/mtk_btif.c:1755`) → `hal_btif_raise_wak_sig` (`D/btif/mt6582/hal_btif.c:969-993`):
   - refuses if the BTIF clock is off (974-978) — **the clock must be on before the pulse**;
   - `BTIF_WAK` (`btif + 0x64`, write-only, `D/btif/mt6582/inc/hal_btif.h:32,79`): clear bit 0
     ("pull ap_wakeup_consys low"), `usleep_range(64, 96)` ("longer than 1/32k period"), set bit 0.
2. The raw `0xFF` WAKEUP_CMD is compiled out (`#if 0`, 1290-1300).
3. `wmt_core_rx()` for WAKEUP_EVT `02 03 02 00 00 03`, 2000 ms; timeout → firmware assert (reason 34).

**[ELF]** stock `opfunc_pwr_sv` @0xc03f66ac calls `wmt_core_tx` twice (SLEEP, HOST_AWAKE) and
`wmt_core_ctrl` once (WAKEUP), matching the source.

**"Host awake" (chip-initiated wake) uses the same WAKEUP sequence, not HOST_AWAKE_CMD:**
`wmt_lib_ps_do_host_awake()` is `#if 1 return wmt_lib_ps_action(WAKEUP)`
(`D/common/core/wmt_lib.c:644-650`). **[ELF]** confirmed: stock `wmt_lib_ps_handler` @0xc03fce84,
HOST_AWAKE branch `c03fd0f8: mov r0,#2 ; bl wmt_lib_ps_action` (2 = `WAKEUP`, enum in
`D/common/core/include/stp_wmt.h:62-69`: SLEEP=0, HOST_AWAKE=1, WAKEUP=2, EIRQ=3, ROLL_BACK=4).
So HOST_AWAKE_CMD/EVT are dead on the Y2.

### 1.4 Idle period and enabling

- Idle-to-sleep: `STP_PSM_IDLE_TIME_SLEEP` = **5000 ms** (`D/common/core/include/psm_core.h:60`),
  `gPsIdleTime` (`wmt_lib.c:84`), timer armed by `_stp_psm_start_monitor` (`psm_core.c:676-695`,
  `osal_timer_modify` at 692). Debug ioctl can set 1–20000 ms (`D/common/linux/pri/wmt_dev.c:303-318`).
- `gPsEnable = 1` (`wmt_lib.c:85`); `CFG_WMT_PS_SUPPORT 1` (`D/common/include/wmt_plat.h:64`).
- PSM starts **disabled** (`stp_psm_init` → `_stp_psm_disable`, `psm_core.c:2130`) and is enabled by
  the first `ENABLE_PSM_MONITOR()` after any WMT operation. Every WMT op (function on/off, etc.) is
  wrapped `DISABLE_PSM_MONITOR()` (force wake-up + disable monitor) / `ENABLE_PSM_MONITOR()`
  (`wmt_lib.c:150-175`; e.g. `D/common/linux/pri/wmt_exp.c:119-129`).
- `mtk_wcn_stp_psm_enable` only enables when STP is ready **and** in BTIF full-set mode
  (`stp_core.c:1520-1531`) — never during mandatory-mode bootstrap.
- Timeouts: PSM thread waits for the WMT result up to `STP_PSM_WAIT_EVENT_TIMEOUT` 6000 ms
  (`psm_core.h:62`, `psm_core.c:2111`); a forced wake (`_stp_psm_do_wakeup`, 1473-1541) polls for ACT
  every 20 ms up to 2000 ms (`_stp_psm_do_wait`, 1444-1471) and retries the WAKEUP op up to 10 times.

### 1.5 "Quick sleep" (screen off)

`_stp_psm_is_quick_ps_support()` is true when `g_quick_sleep_ctrl` (default 1, `wmt_lib.c:91`) and
the early-suspend flag (screen off) are set and the IC says yes — SOC returns TRUE
(`D/common/core/wmt_ic_soc.c:1358-1361`; `wmt_lib.c:814-824`). Then:
- after **every** non-WMT STP send, a SLEEP op is queued immediately (`stp_core.c:2950-2960`);
- after every wake-up completes, a SLEEP is queued instead of arming the 5 s timer (`psm_core.c:1026-1029`);
- a 1 s traffic sampler (`stp_psm_disable_by_tx_rx_density`, `psm_core.c:1705-1740`, called on TX
  `stp_core.c:2717-2723` and RX `stp_core.c:3111-3117`) sets `DISABLE_MONITOR_TX_HIGH_DENSITY` when
  RX+TX > 50 000 B/s (`psm_core.h:203-204`), which makes `_stp_psm_notify_wmt_sleep_wq` ignore sleep
  requests (`psm_core.c:892-905`) until the next sample is below threshold or the idle timer fires
  (`_stp_psm_stp_is_idle` clears both density flags, 1363-1379).
- The early-suspend hook in the donor port was replaced by a PM notifier (`wmt_dev.c:218-246`);
  stock is `CONFIG_EARLYSUSPEND`. **Unconfirmed** in the ELF (no early-suspend symbols found;
  `stp_psm_disable_by_tx_rx_density` @0xc0403378 does exist).

With the screen on, only TX restarts the 5 s timer (`stp_core.c:2778`); the RX-side restart is
inside `#if 0` (`stp_core.c:1683-1693`).

### 1.6 State machine (`psm_core.h:108-113`, `psm_core.c`)

States: `ACT`, `ACT_INACT` (going to sleep), `INACT` (asleep), `INACT_ACT` (waking).
Any state other than ACT sets `STP_PSM_BLOCK_DATA_EN` (`_stp_psm_set_state`, `psm_core.c:645-674`).
All ops run in the `mtk_stp_psm` kthread under `stp_psm_lock` (`_stp_psm_handler`, 101-145), which is
the same lock every STP sender takes (`stp_core.c:2732, 2764`).

| From | Event | Action |
| --- | --- | --- |
| ACT | idle timer → SLEEP op | → ACT_INACT; flush held FIFO; queue WMT SLEEP (async); wait ≤6 s (1250-1271) |
| ACT_INACT | WMT callback SLEEP | `SLEEP_EN` → INACT, `mt_combo_plt_enter_deep_idle(COMBO_IF_BTIF)` (gate clocks, 1038), wake_unlock (1031-1044) |
| ACT_INACT | ROLL_BACK (STP not ready) | release data → ACT (1045-1056) |
| INACT | TX → WAKEUP op | → INACT_ACT, wake_lock, `mt_combo_plt_exit_deep_idle` (ungate, 1301), WMT WAKEUP, wait (1290-1312) |
| INACT | wake IRQ → EIRQ → HOST_AWAKE op | same as WAKEUP (1313-1335; the WMT side sends WAKEUP, see 1.3) |
| INACT_ACT | WMT callback WAKEUP/HOST_AWAKE | `WAKEUP_EN` → release held data (≤20 packets per call), clear BLOCK, ACT, then quick-sleep or arm timer (1012-1030) |
| INACT_ACT | ROLL_BACK | → INACT (1057-1060) |
| ACT | WAKEUP/HOST_AWAKE | ignored, release data (1272-1276) |
| INACT | SLEEP | ignored (1336-1339) |

Op-queue de-duplication drops repeated SLEEP and WAKEUP/HOST_AWAKE pairs (221-289) and cancels
opposite pairs (291-335).

**TX while asleep or in transition** (`stp_core.c:2738-2748`, `2770-2795`): the sender takes
`stp_psm_lock` (so it blocks while a transition holds it), then if `BLOCK_DATA_EN` is set the frame is
copied into an 8 KiB hold FIFO (`_stp_psm_hold_data`, `psm_core.c:714-756`; `STP_PSM_FIFO_SIZE`
`psm_core.h:48`), a WAKEUP op is queued and the send returns "success". After wake-up the FIFO is sent
with `stp_send_data_no_ps` (797). **Hazards in stock:** a full FIFO is *reset* (data dropped, 739-742);
a corrupt delimiter resets it too (803-805); only 20 packets are released per call.

---

## 2. Chip → host wake while BTIF is gated

- Line: CONN2AP BTIF wakeup ("BGF") interrupt, **GIC SPI 185** (`D/plat/include/consys_res.h:23`;
  stock snapshot shows it as Linux IRQ217 "BTIF_WAKEUP_IRQ", `docs/knowledge/m5-connectivity-entry.md:57`).
  Our DTS already has it as `"wake"`, `<GIC_SPI 185 IRQ_TYPE_LEVEL_LOW>` (`kernel/dts/innioasis-y2.dts:319-322`).
- Stock handling (`D/plat/wmt_plat_alps.c`):
  - `request_irq(..., IRQF_TRIGGER_LOW, "BTIF_WAKEUP_IRQ")` at CONSYS power-on, then immediately
    disabled (`D/plat/mtk_wcn_consys_hw.c:330-331`; `wmt_plat_alps.c:507-519`).
  - **Enabled only when going to sleep**: `wmt_plat_eirq_ctrl(PIN_BGF_EINT, PIN_STA_EINT_EN)` right
    after the SLEEP op is queued (async, so *before* SLEEP_CMD is even sent) (`wmt_lib.c:682-684`).
    **Disabled before a host-initiated wake** (`wmt_lib.c:714-717`). **[ELF]** order confirmed in
    `wmt_lib_ps_handler` (sleep: `bl wmt_lib_ps_action` @c03fcfdc then `eirq_ctrl(0,5)` @c03fcff8;
    wake: `eirq_ctrl(0,6)` @c03fd094 then `ps_action(2)`).
  - ISR `wmt_plat_bgf_irq_isr` (287-295): `disable_irq_nosync` (counter-balanced, 536-550), then the
    callback `wmt_lib_ps_irq_cb` → EIRQ → PSM HOST_AWAKE op. **No acknowledge register**: the line is
    level-low and is simply masked until the next sleep. When CONSYS releases it is **unconfirmed**
    (presumably after the WAKEUP handshake).
- BTIF RX does **not** work with clocks gated: register access needs the clock
  (`hal_btif_raise_wak_sig` refuses with the clock off, `hal_btif.c:974-978`; every stock BTIF/DMA
  IRQ handler brackets its register access with a clock reference, `D/btif/mtk_btif.c:1004-1088`).
  The protocol guarantees the chip does not transmit while the host is in INACT: it raises the wake
  line instead and sends only after the host ungates + pulses BTIF_WAK. **Unconfirmed** whether the
  chip's queued data arrives before or after WAKEUP_EVT (the stock host does not care: any non-WMT
  data is routed by channel).
- Deep idle: SPI 185 is hwirq 217 = CIRQ bank 4 bit 25, inside the valid mask `0x07ffffff`
  (`kernel/platform/cirq-policy.h:6-10`), and the deep-idle SPM wake mask includes `WAKE_SRC_CIRQ`
  (`kernel/platform/spm-idle-policy.h:42`), so it should wake deep idle — **not verified on device**.
  Slow idle is plain WFI (`kernel/platform/clocks.c:156-194`), any GIC IRQ wakes it.

---

## 3. What the host does to BTIF and its DMA channels

Call chain for "enter/exit deep idle": `mt_combo_plt_{enter,exit}_deep_idle(COMBO_IF_BTIF)`
(`D/detect/mtk_wcn_stub_alps.c:288-305, 322-337`) → `wmt_plat_deep_idle_ctrl`
(`D/plat/wmt_plat_alps.c:254-264`) → `mtk_wcn_consys_stp_btif_dpidle_ctrl` (`stp_btif.c:224-236`) →
`mtk_wcn_btif_dpidle_ctrl` (`D/btif/mtk_btif_exp.c:296-312`) → `btif_enter_dpidle` / `btif_exit_dpidle`
(`D/btif/mtk_btif.c:1626 / 1590`, under the BTIF state mutex).

**Enter** (`_btif_enter_dpidle_from_on`, `mtk_btif.c:1679-1730`):
1. Poll `_btif_is_tx_complete` every 1 ms for up to 5000 ms (1695-1705). Complete means
   (`mtk_btif.c:1772-1795`, `hal_btif.c:1085-1124`, `hal_btif_dma.c:754-787`):
   TX DMA `VFF_VALID_SIZE` (+0x3c) == 0 **and** TX DMA `INT_BUF_SIZE` (+0x38) == 0;
   BTIF `LSR` (+0x14) TEMT (bit 6) set, **RX DR (bit 0) clear**, `IER` TXEEN (bit 1) clear;
   the software TX kfifo empty.
2. Then **only gates clocks**: TX-DMA clock off, RX-DMA clock off, BTIF clock off (1707-1722).
   Both DMA "clocks" are one reference-counted AP_DMA CG (`MT_CG_PERI_AP_DMA`,
   `hal_btif_dma.c:181-240`, `D/btif/mt6582/inc/hal_btif_dma.h:20`); BTIF is `MT_CG_PERI_BTIF`
   (`hal_btif.c:224-300`, `hal_btif.h:19`); ref counting on (`D/btif/hal_inc/hal_pub.h:39-40`).
3. On a 5 s timeout it returns -1 and leaves clocks on (the PSM state still becomes INACT).
- **No DMA stop, no channel disable, no IRQ masking, no register save/restore.** The BTIF/AP_DMA
  registers stay in an always-on domain during slow/deep idle; DMA pointers and enables are retained.
  `btif_sleep_ctrl` (`BTIF_SLEEP_EN` +0x48) exists but is **never called** (`hal_btif.c:482-499`).

**Exit** (`_btif_exit_dpidle_from_dpidle`, `mtk_btif.c:1637-1658`): enable TX-DMA, RX-DMA, BTIF clocks
(in that order). Nothing else. `_btif_send_data` also exits deep idle on its own if a send arrives
while gated (`mtk_btif.c:2635-2665`) — a safety net.

`btif_clock_ctrl` (`mtk_btif.c:1390-1405`, the ledger's F37 symbol) is the same three clock calls.

**Ordering constraints:**
- Enter: SLEEP_EVT received → (our STP ACK for it must be out) → TX quiet + RX FIFO empty → gate.
- Exit: ungate **before** `BTIF_WAK` pulse (pulse needs the clock) → wait WAKEUP_EVT.
- Stock wake IRQ: enabled before sleep, disabled before host wake / in its ISR.
- System suspend: stock `_btif_suspend` refuses while the BTIF is ON (data pending) and only frees the
  controller from DPIDLE (`mtk_btif.c:306-350`). Y2 powers radios off before suspend instead
  (`kernel/platform/connectivity/core.c:343-349`), so this does not carry over.

---

## 4. SOC vs combo, per-function conditions

- The same `psm_core.c` drives both. On SOC (MT6582) the differences are: WAKEUP is a BTIF register
  pulse instead of the `0xFF` UART byte (`wmt_core.c:1284-1300`), the host-awake path uses WAKEUP
  ([ELF]), the wake line is the CONN2AP SPI (not a GPIO EINT), and quick sleep is always supported
  (`wmt_ic_soc.c:1358-1361`). SDIO "own" handling (`wmt_lib.c:687-707`) is not used.
- **No per-function condition in the PSM**: nothing disables PSM while Wi-Fi or FM is on, or while A2DP
  streams. Wi-Fi does not use STP/BTIF for data on MT6582 (separate WLAN interface), so with Wi-Fi only
  the BTIF still sleeps. FM drivers do not touch PSM (no hits in `D/fmradio`). Only WMT ops (function
  on/off, etc.) temporarily disable/re-enable it (1.4); firmware assert/coredump disables it
  (`stp_core.c:481`, sleep refused while coredump `wmt_lib.c:662-666`).
- What keeps it awake during activity: any STP TX restarts the 5 s timer (screen on). A2DP sends
  every few ms, so streaming never reaches 5 s idle. With the screen off (quick sleep) the chip is put
  to sleep after every send unless the 1 s sample exceeds 50 kB/s; SBC/AAC A2DP (≈32–43 kB/s) is below
  that, so stock would cycle sleep/wake during dark playback; whether the op de-duplication actually
  lets that happen per packet is **unconfirmed**.
- RX does not restart the timer in screen-on mode (only TX). The chip-side BT controller keeps its
  links while the host interface sleeps (implied by stock behaviour; **unconfirmed** in documentation).

---

## 5. Comparison with our driver and proposed design

### 5.1 What we have / what is missing

| Item | Our driver | Missing |
| --- | --- | --- |
| Clocks | `clk_bulk_prepare_enable(2, &c->clocks[1])` at `y2_btif_start` (`btif.c:130`), released only in `y2_btif_stop`/`y2_btif_abort` (`btif.c:189, 235`) | runtime gate/ungate |
| Wake IRQ (SPI 185, `irq[3]`) | requested with the shared handler, **enabled for the whole transport life** (`btif.c:145, 212`); handler only writes `BTIF_WAK = 1` (`btif.c:48-51`) | enabled only in INACT, disable-in-handler, wake work |
| BTIF_WAK pulse | none (only the write of 1) | 0 → 64–96 µs → 1 |
| SLEEP / WAKEUP WMT messages | none | SLEEP_CMD/EVT, unsolicited WAKEUP_EVT wait |
| State machine / idle timer | none | ACT/ACT_INACT/INACT/INACT_ACT + delayed work |
| TX gate | `y2_stp_send` (`stp.c:47-86`) is the single TX entry for WMT/BT/FM (`hci.c:96`, `fm.c:100`, `wmt.c:23`); ACKs from `y2_stp_receive` (`stp.c:166-168`); retransmits from `y2_stp_retry` (`stp.c:11-29`) | wake-before-send, block retry/ACK while gated |
| Register access paths that would fault/hang while gated | TX-DMA IRQ (`btif.c:37-46`), RX path (`btif.c:52-75`), `y2_btif_report_timeout` (149-166), `y2_btif_stop` (168-193), `y2_btif_abort` (220-239) | all must run with clocks on |
| BTIF/APDMA idle counters | `slow_reject_clock`, `slow_blocker_counts` bits 11/20, `deep_peri_blockers` (`clocks.c:109-134`) | PSM's own counters |

### 5.2 Minimal design (switch `psm`, default off)

Module parameters (in `core.c` or a new `psm.c`): `psm` (bool, default N, read when the transport
reaches full STP, 0644), `psm_idle_ms` (default 5000, clamp 100–20000 like stock).
No quick sleep in the first version (screen-off per-packet sleep is the riskiest stock behaviour and
needs the measured wake latency first).

New state in `struct y2_conn` (`conn.h`): `enum psm_state {OFF, ACT, ACT_INACT, INACT, INACT_ACT}`,
`struct mutex psm_lock`, `struct delayed_work psm_idle`, `struct work_struct psm_wake`,
`bool btif_clocks_on`, `bool psm_waiting_event`, counters (5.4). Lock order:
`psm_lock` → `command` → `stp_lock` → `dma_tx`/`dma_rx` (matches today's `y2_conn_wmt` → `y2_stp_send`
→ `y2_btif_send`).

**Activation:** after `y2_wmt_boot` succeeds (full STP verified, `wmt.c:142-145`) and `psm` is set,
state = ACT, arm idle. Any WMT op from `y2_conn_get/put` (`core.c:163-202`) runs with PSM forced ACT
(as stock's DISABLE/ENABLE_PSM_MONITOR): take `psm_lock`, wake if needed, run, re-arm.

**TX gate (synchronous, no hold FIFO):** at the top of `y2_stp_send` for channels BT/FM/WMT (all
callers are process context: `hci.c` tx_work, `fm.c`, `wmt.c`):
`y2_psm_get(c)` = `mutex_lock(psm_lock)`; if INACT → host wake (below); `psm_busy++`;
`mutex_unlock`. After the send: `y2_psm_put(c)` → re-arm `psm_idle`. Internal PSM messages bypass the
gate. This replaces stock's 8 KiB hold FIFO (which drops data on overflow) with blocking, which
`y2_stp_send` already does (1500 ms window wait).

**Idle → sleep (`psm_idle` work):**
1. `mutex_lock(psm_lock)`; refuse (count reason, re-arm) unless: state ACT, `transport_on`, `full_stp`,
   no `failure`/`recovering`, `stp_pending == 0` (stronger than stock: no unacked frames),
   `skb_queue_empty(&c->hci_tx)`, `rx_used == 0`, `mutex_trylock(&c->lifecycle)` succeeds (no
   function on/off or boot in progress; drop it again after the check).
2. state = ACT_INACT; `y2_conn_wmt(c, 01 03 01 00 01, expect 02 03 02 00 00 01)` (2000 ms is the
   stock limit; ours is 4000 ms, `wmt.c:25`). On failure: count, `y2_conn_failed()` (stock asserts the
   firmware; the existing recovery path is our equivalent) — do **not** gate.
3. `enable_irq(c->irq[3])` (wake line armed before gating; it is level-low, so an assertion that
   happens before this point is still seen).
4. `mutex_lock(dma_rx)` so the STP ACK for SLEEP_EVT (sent from the RX thread, `stp.c:166-168`) has
   been queued; then poll ≤ 50 ms (stock: ≤ 5 s, 1 ms) for: TX `VALID`(+0x3c) == 0,
   TX `INT_BUF_SIZE`(+0x38) == 0, TX `FLUSH`(+0x14) bit 0 clear, BTIF `LSR`(+0x14) TEMT set and DR
   clear, RX `WRITE_PTR`(+0x2c) == `c->rxptr`, RX `VALID`(+0x3c) == 0.
5. If quiet: `disable_irq()` the BTIF/TX-DMA/RX-DMA IRQs (synchronous, `irq[0..2]`),
   `clk_bulk_disable(2, &c->clocks[1])` (keep prepared), `btif_clocks_on = false`. If not quiet:
   keep clocks on, state INACT anyway (chip asleep, host still clocked; harmless), count `gate_busy`.
6. state = INACT; `mutex_unlock(dma_rx)`; `mutex_unlock(psm_lock)`.

**Host wake (from the TX gate or `psm_wake`), under `psm_lock`:**
1. state = INACT_ACT; `disable_irq(c->irq[3])` (or already masked by the handler).
2. If gated: `clk_bulk_enable`, `enable_irq(irq[0..2])`, `btif_clocks_on = true`; check RX
   `WRITE_PTR` vs `rxptr` (count `rx_while_gated` if data landed — this is the lost-RX detector).
3. `mutex_lock(&c->command)`; `reinit_completion(&c->response)`; `psm_waiting_event = true`;
   pulse: `writel(0, btif+0x64); usleep_range(64, 96); writel(1, btif+0x64)`;
   `wait_for_completion_timeout(2000 ms)`; compare with `02 03 02 00 00 03`.
4. Success → state ACT, record latency, re-arm idle. Timeout → one more pulse (stock retries the WAKEUP
   op up to 10×, `psm_core.c:1477-1533`), then `y2_conn_failed(-ETIMEDOUT)`.

**Chip wake:** give `irq[3]` its own threaded handler: `disable_irq_nosync(irq)`, `pm_wakeup_event`,
`schedule_work(&c->psm_wake)`; if state is ACT, count `wake_irq_spurious` and return (stock ignores it).
The work runs the host-wake sequence (stock does exactly that, [ELF]).

**`deliver()` filter (`stp.c:102-122`):** the PSM events `02 03 02 00 00 0x` must only complete
`c->response` while `psm_waiting_event`/the SLEEP command is outstanding; otherwise drop and count
(`wmt_event_unsolicited`). Today an unexpected second WMT event returns -EPROTO
(`stp.c:118`) and fails the whole transport.

**Teardown:** `power_off`/`recover_locked`/`y2_btif_stop`/`y2_btif_abort`/`y2_btif_report_timeout`
first call `y2_psm_force_clocks_on()` (ungate + IRQs, no chip handshake needed when powering off),
cancel `psm_idle`/`psm_wake`, `disable_irq(irq[3])`, state OFF, so the clock refcount stays balanced
(`btif.c:189, 235` unprepare exactly once). `y2_stp_retry` must not write while gated: it only runs
with `stp_pending > 0`, and sleep requires `stp_pending == 0`; still assert `btif_clocks_on` in
`y2_btif_send` and return -EIO + count if violated.

### 5.3 Riskiest points

1. **Lost RX**: chip sending into a gated BTIF/AP_DMA. Mitigation: gate only after SLEEP_EVT and an
   empty RX FIFO/DMA; `rx_while_gated` counter on ungate; keep the existing ack-before-drain order
   (`btif.c:53-60`).
2. **Wake races**: wake IRQ during ACT_INACT (between SLEEP_CMD and SLEEP_EVT) or concurrent with a TX
   wake; both serialize on `psm_lock`, and the level-low line stays asserted until handled. Unknown:
   whether the chip can refuse/abort a sleep (stock has no NAK path; SLEEP_EVT status byte is always 0).
3. **WMT event demultiplexing**: WAKEUP_EVT is unsolicited from our side (we pulse a register, not
   send a command); it must not collide with another WMT command — hold `c->command` across the wait.
4. **BT audio**: wake latency (pulse + chip wake + one STP frame; unmeasured) on the first packet after
   ≥ 5 s idle (pause→play, first skip). Streaming itself never idles 5 s. Known open issue "supervision
   timeout ~22 s after every pause" (radio daemon notes) must be re-checked with PSM on: if the
   controller loses links while BGF sleeps, PSM would make it worse.
5. **Clock sharing**: AP_DMA CG is shared with I2C DMA; using our own CCF reference (as today) is
   safe; never touch the CG directly.
6. **Deep idle still blocked**: POWER-FINAL-32 lists "CONN domain" as a further deep-idle blocker; PSM
   removes BTIF/APDMA (the measured slow-idle blocker: 376 058 of 376 089 refusals) but not
   necessarily the deep-idle refusal. Expect slow-idle (bus DCM) gains first.
7. **Firmware behaviour on a failed handshake**: stock asserts the firmware; we recover the whole
   transport — that is visible to BlueZ (link drop) but bounded.

### 5.4 Observation on the device (counters to expose)

New read-only sysfs attribute `psm` (or extend `status`, `core.c:312-325`):
`state`, `enabled`, `idle_ms`, `sleeps`, `sleep_refused_{pending,queue,rx,busy,lifecycle}`,
`sleep_failures`, `gate_busy` (quiet check timed out), `host_wakes`, `chip_wakes`,
`wake_irq_spurious`, `wake_failures`, `wake_retries`, `wake_latency_us_{last,max}`,
`asleep_ms` (cumulative ktime in INACT), `rx_while_gated`, `send_while_gated`,
`wmt_event_unsolicited`.

Existing counters to compare before/after: `/sys/module/.../slow_reject_clock`,
`slow_blocker_counts` (bit11 APDMA, bit20 BTIF), `deep_peri_blockers`, `dormant_entries`
(`kernel/platform/clocks.c:109-134`); `/proc/interrupts` line for SPI 185 ("wake") — with the current
always-enabled handler its count tells whether CONSYS ever asserts the line today (should be 0);
`status` `transport_errors`/`recoveries`.

Device test matrix (with `psm=1`, owner-authorised build): BT on, no link (sleeps after 5 s, slow
idle no longer refused for bits 11/20); `bluetoothctl` command while asleep (host wake);
incoming page/AirPods self-reconnect while asleep (chip wake); A2DP play/pause/resume/skips; long
paused link (> supervision timeout); FM on; Wi-Fi on + BT idle; BT off/on and recovery while asleep;
system sleep/resume cycle. Pass = `transport_errors` and `recoveries` unchanged,
`rx_while_gated = 0`, `wake_failures = 0`.

---

## Unconfirmed items (summary)

- When CONSYS deasserts the CONN2AP wake line; whether its queued data precedes or follows WAKEUP_EVT.
- Whether the chip can transmit between SLEEP_EVT and host gating (stock assumes not).
- Stock early-suspend flag wiring (quick sleep) — source only, not seen in the ELF.
- Whether quick sleep really cycles per A2DP packet in stock.
- SPI 185 waking SPM deep idle via CIRQ on the device.
- Whether "CONN domain" still blocks deep idle once BTIF/APDMA are gated.
- That the BT controller keeps ACL links while the host interface is in PSM (implied by stock use).
