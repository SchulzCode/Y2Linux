# Y2 storage hardware ceiling — eMMC and SD bus modes

<!-- knowledge-base-scope: scoped-validation-record -->
> **Source and software record, 2026-10-01.** The ceilings below come from
> stock Y2 binaries, the MT6582 BSP source, the installed eMMC's EXT_CSD and
> the MT6323 regulator tables. The implementation is software-validated.
> **No bus mode above 50 MHz has run on a Y2 under Linux yet.** The candidate
> section says what was built. Physical results belong in a separate report.

Before this pass both hosts ran at 50 MHz SDR. The eMMC used 8-bit HS52 and
the SD used 4-bit SD High Speed at 3.3 V. That limit came from our own
device tree, not from the hardware.

## Answers

| Question | Answer | Basis |
| --- | --- | --- |
| Can the eMMC go beyond 50 MHz? | **Yes** | HS200 at 200 MHz: host source 200 MHz, card type 0x57, IO fixed 1.8 V, stock msdc0 flags include `MSDC_UHS1` (BSP maps it to the HS200 capability) |
| Is DDR52 possible? | **Yes** | Card type bit 2 (DDR52 1.8/3 V); stock msdc0 flags include `MSDC_DDR`; DDR clock = source/4 = 50 MHz |
| Is 100 MHz / HS200 possible? | **Yes, HS200 at 200 MHz** | Mainline CMD21 tuning on PAD_TUNE/IOCON (MSDC v4, `mt8135_compat`). An HS200 bus held at 100 MHz (`y2_clock_limit_hz`) moves as many bytes as DDR52, so the ladder falls from 200 MHz HS200 straight to the untuned DDR52 |
| Is HS400 possible? | **No** | The card supports it (bit 6), but MT6582 MSDC has no data strobe / EMMC50 block |
| Can SD use 100 MHz? | **Yes** | SDR50 runs at source/2 = 100 MHz |
| Is SDR50 possible? | **Yes** | Stock msdc1 flags include `MSDC_UHS1` → SDR50/SDR104 |
| Is 1.8 V UHS wired? | **Yes** | Stock `msdc_sd_power_switch` (0xc04f2b60) sets MT6323 **VMC** to 1800 mV and applies dedicated 1.8 V pad drive values; VMC's table is {1.8, 3.3 V} |
| Is SDR104 possible? | **Yes, at 200 MHz** | The 200 MHz source is undivided. The spec allows up to 208 MHz, so 200 MHz is the host maximum. CMD19 tuning |

These answers depend on the card that is inserted. A card without UHS-I, or
one that refuses 1.8 V, negotiates SD High Speed at 3.3 V as before.

## Evidence

| Item | Finding | Source |
| --- | --- | --- |
| msdc0 board flags | `0x3c0` = SYS_SUSPEND, HIGHSPEED, UHS1, DDR; 8 data pins; drive clk/cmd/dat 4/2/2 | hash-identified stock kernel `msdc_hw` record, `out/platform-v1-physical-bringup/stock-reference/msdc-board-platform-data.json` |
| msdc1 board flags | `0x3e3` = CD, WP, REMOVABLE, SYS_SUSPEND, HIGHSPEED, UHS1, DDR; 4 pins; drive 7/7/7 at 3.3 V, sd_18/sdr50/ddr50 drive 4 | same |
| Flag mapping | UHS1 → `MMC_CAP_UHS_SDR12/25/50/104` + `MMC_CAP2_HS200_1_8V_SDR`; DDR → `UHS_DDR50`, `1_8V_DDR` | MT6582 BSP `mt6582/sd.c` |
| eMMC power | Stock powers only VEMC_3V3 (card VCC, 3.3 V). The VIO18 line is commented out: eMMC IO is the always-on 1.8 V VIO18 rail. `msdc_ops_switch_volt` returns 0 for eMMC | BSP `msdc_emmc_power`, stock disassembly |
| eMMC IO range | DVDD18_MSDC0 1.7–1.95 V; MediaTek's own reference board uses vqmmc = vio18 | MT6592 datasheet, `mt8173-evb.dts` |
| SD power | VMCH 3.3 V card supply, VMC IO supply 3.3 V → 1.8 V on switch; then Schmitt trigger, RDSEL/TDSEL (0 in both modes on MT6582) and the 1.8 V drive table; 10 ms after power-on and power-off | stock `msdc_sd_power`, `msdc_sd_power_switch`, `msdc_set_driving` (0xc04f28c0) |
| EN18IO comparator | Absent from MT6582 (`REMOVEED_FOR_MT6582`, no stock symbol) | BSP, stock symbols |
| Pad drive cells | GPIO 0x10005000, bits 10:8; MSDC0 0xc00/0xc10/0xc20, MSDC1 0xc40/0xc50/0xc60 | stock `msdc_set_driving` |
| Source clock | LK selector 1: MSDCPLL 400 MHz / 2 = 200 MHz; undivided 200, /2 = 100, DDR = src/4 = 50 MHz | `kernel/platform/clocks.c`, LK |
| eMMC card | EXT_CSD[196] = 0x57: HS26, HS52, DDR52, HS200 1.8 V, HS400 1.8 V | identification readback |
| Stock tuning | Error-driven retune (`msdc_tune_cmdrsp/read/write`, AUTO_K), dropping modes after power-cycle failures. Mainline's CMD19/21 sweep uses the same PAD_TUNE/IOCON fields | stock disassembly, mainline `msdc_execute_tuning` |

Extracted disassembly is kept privately in `out/storage-ceiling-source/stock/`.

## Implementation

| Part | Source | What it does |
| --- | --- | --- |
| Ladders and policy | `kernel/platform/storage-modes.h` | Ladder levels, card-capability parsers, fallback rule, EXT_CSD identity bytes, stock pad drive table |
| Driver | `kernel/patches/0009-y2-msdc-readonly.patch` (`mtk-sd.c`) | Per-level caps (DT caps ∩ level) and clock cap; voltage switch with VMC and pad drive; tuning record; fault classification; fallback worker; post-negotiation readback; `y2_storage` status |
| Pad drive | `kernel/platform/pinctrl.c` `y2_msdc_pad_drive()` | The GPIO block owner writes the stock 3.3 V or 1.8 V drive values under its lock; probe returns `-EPROBE_DEFER` until pinctrl is up |
| Request firewall | `kernel/platform/storage-policy.h` | Admits CMD21 (tuning block read, no address) and the CMD6 values HS200 needs: HS_TIMING ≤ 2 with default drive strength, BUS_WIDTH 5/6 (DDR). HS400 and drive-type changes stay denied |
| Device tree | `kernel/dts/innioasis-y2-production.dts` | mmc0: `mmc-ddr-1_8v`, `mmc-hs200-1_8v`, vqmmc = VIO18 (fixed, always-on), **no vmmc**. mmc1: all UHS-I modes, vmmc = VMCH 3.3 V, vqmmc = VMC 1.8–3.3 V. Rails exist only in the production DT, each with a consumer |
| Status | `tools/platform/y2_platform/observe.py` | `storage.controllers[].mode` carries the decoded `y2_storage` line |
| Reborn | `crates/reborn-platform/src/client.rs` (Reborn `b92d312`) | Diagnostics → Storage shows mode, card, tuning and faults per host |

### Negotiation and fallback

The MMC core negotiates the highest mode both the host and the card admit.
The current ladder level limits what the host offers.

| Host | Ladder (highest → lowest) |
| --- | --- |
| eMMC | HS200 200 MHz → DDR52 50 MHz DDR → HS52 50 MHz → HS 25 MHz → legacy 13 MHz |
| SD | SDR104 200 MHz → DDR50 50 MHz DDR → SDR50 100 MHz → HS 50 MHz 3.3 V → HS 25 MHz → legacy 13 MHz |

The SD ladder follows the core's own preference, SDR104 > DDR50 > SDR50, so
SDR50 comes after DDR50. A 1.8 V switch failure skips every remaining UHS
level.

| Trigger | Action |
| --- | --- |
| Tuning failure (CMD19/21) | `tuning` event; the next level is applied to the caps. The core then falls back on this init |
| 1.8 V switch failure (SD) | `voltage` event; skip to 3.3 V HS |
| CRC / timeout at a tuned timing | The core retunes first; a second strike at the same level downgrades |
| CRC / timeout at an untuned timing, controller data error | Immediate downgrade |
| Readback mismatch | `verify` event, downgrade with card re-init. 500 ms after any high timing is set, eMMC re-reads EXT_CSD and compares the identity bytes the core itself treats as read-only (plus SEC_COUNT). SD reads SD_STATUS twice; both copies must match and report 4-bit width |

A downgrade lowers the caps first. If the current timing is still admitted,
only the clock changes. Otherwise the card is re-initialised with
`mmc_hw_reset()`. The core flushes the cache first. On eMMC its "power
cycle" changes no rail (no vmmc, VIO18 always-on), so the card sees a CMD0
re-initialisation. SD is power-cycled through VMCH. If re-init fails, the next level is tried. The
filesystem never stays on a mode that failed.

### Safety

- Root/data eMMC is never power-cycled. VEMC_3V3 is not declared, so Linux
  cannot switch the card supply off. VIO18 is always-on.
- No IO voltage is guessed. eMMC IO stays at the fixed 1.8 V rail. SD uses
  only VMC's two table values, which are the values stock uses.
- Partitions, geometry, boot areas and protected regions are untouched. The
  request firewall admits only the CMD21 block read and the HS200/DDR switch
  values.
- CRC and error handling are unchanged. Retries and fallbacks are added on
  top.
- Runtime PM keeps MSDC_CFG, IOCON, PAD_TUNE and DAT_RDDLY0/1 across clock
  gating, so tuning survives. eMMC s2idle re-initialises on resume, which
  re-negotiates and retunes.
- SD hotplug: CMD55 timing out with no card resets the SD ladder to its
  ceiling, so a replacement card starts from the top. VMC/VMCH power-off is
  followed by the stock 10 ms wait.

### Boot controls

| Parameter | Effect |
| --- | --- |
| `y2.emmc_mode=<level>` | eMMC ceiling, e.g. `HS52` for the previous behaviour |
| `y2.sd_mode=<level>` | SD ceiling, e.g. `HS` for 3.3 V 50 MHz |
| `y2.mmc_safe=1` | Existing safe storage mode: 26 MHz source, legacy 13 MHz, no high-speed caps |

`y2_clock_limit_hz` also accepts 13/25/50/100/200 MHz at runtime.

## Diagnostics

`/sys/bus/platform/devices/1123{0,4}0000.mmc/y2_storage` is one line of
`key=value`:

`host level ceiling timing width clock_hz actual_hz cap_hz signal_mv vqmmc_mv
card_level card_caps hs400_card tuning tuning_runs tuning_failures tune_iocon
tune_pad tune_rddly0 voltage_switches verify fallbacks resets reset_error
clock_error drive_before crc_events timeout_events controller_events
tuning_events voltage_events verify_events`

`y2-platform status` exposes it as `storage.controllers[].mode`, and Reborn
Diagnostics shows it as plain text. No register decoding is needed.

## Tests

| Test | Covers |
| --- | --- |
| `tests/test_storage_ceiling.py` (11) | Ladder invariants, card parsing (EXT_CSD type, SD bus modes), pad table, caps never exceed DT, safe mode, fallback with re-init and repeated failure, fault classification, retune strikes, voltage/tuning hooks, SD_STATUS readback, empty-slot reset, runtime-PM retained registers incl. DDR, status parsing, DT rails |
| `tests/test_mmc_requests.py` | CMD21 is the only new read; CMD6 183/185 admitted values |
| `tests/test_mmc_context.py` | EXT_CSD identity copy saved at legacy timing only |
| Existing storage and overlay tests | Unchanged contracts still pass |
