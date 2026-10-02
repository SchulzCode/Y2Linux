# Implementation lane: Reborn product and battery

Entry: Linux `9ac2c0b7018ce707ec15f8845daf58ce38ec18a0`, Reborn
`eb64b8b21058eacb1ef183f7570cca8100cbbb46`. Existing dirty Linux documentation
was preserved. No device writes, flash, push or Git identity changes occurred.

## Priority 8 — Product v2

Starting state: Product v2 and the new first-complete-frame splash handoff were
already implemented. EQ existed in FFmpeg but had no product control; sleep
had no typed UI contract; diagnostic export meant private state backup. Codec
selection required `Stopped` although the ordinary UI only exposed Pause.

Implementation:

- Audio → Equalizer uses existing DSP/limiter with five default bands,
  ±12 dB/1 dB wheel editing, Select apply, Back cancel and Reset to Flat.
  Existing custom bands are preserved up to the engine's eight-band limit.
  Reset Settings now rebuilds playback when disabling an active EQ.
- Audio → Output Details exposes existing observed source bits/rate,
  decoder/DSP precision and opened PCM format/rate through Diagnostics.
  No native high-resolution claim is inferred from source or container width.
- System/Quick Settings → Sleep consumes typed `power.sleep` and
  `y2-platform sleep request`, with explicit requested/refused/sleeping/
  restoring/restored/restore-failed states. Active playback is refused with
  actionable wording. Ordinary deep sleep still returns
  `physical_qualification_required` from the platform; owner qualification
  retains its existing explicit path. Same-boot/kernel-completion checks
  protect a restored claim. Short Power still means screen off, preserving
  playback and volume controls.
- Bluetooth presents preference separately from actual negotiated codec.
  Paused/stopped playback permits selection; reconfiguration and the platform
  PCM lease still exclude active streams. SBC quality and LDAC quality/ABR
  controls reflect supported runtime settings; requested/effective settings
  and pending restart remain distinct. Quality labels include both LDAC rate
  families. CLI help and parser tests cover every viable Classic preference.
- Diagnostics exposes a separate redacted report export, immediately shows
  its archive path/retrieval instructions, and labels state export private.
  Failed operations cannot display a successful export message.
- Estimated SOC is named on the Battery page. Long modal lists now account for
  description height, fixing focus rows outside the panel.
- Ready-frame splash, shutdown save/audio stop/SQLite close/black frame/
  backlight off/ack sequence and input duplicate/acceleration/direction-reset
  foundations remain unchanged and included in current software validation.

Files: Reborn `app/reborn/src/{main,ctl}.rs`, preview fixtures;
`crates/reborn-core/src/{lib,platform}.rs`; platform `{client,power}.rs`;
UI `{lib,pages,screens,components,diagnostics,tests}.rs`;
`docs/ui/REBORN-FEATURE-COMPLETION.md` and platform boundary documentation.
No additional direct Y2 filesystem/shell access exists outside reborn-platform.

Validation: fresh fmt check and strict workspace clippy passed. At product
freeze the combined workspace passed **233 tests** (including concurrent audio
agent changes); the parent's final build records any subsequent added tests.
Host preview generated **69** 480×360 states; new EQ/picker/sleep/LDAC cases
were visually inspected. Output is `Y2Reborn/out/feature-completion-previews/`;
workspace log is `Y2Reborn/out/feature-completion-workspace-tests.log`.

Remaining physical proof: actual sleep restoration, new-control wheel and
playback behavior on the panel, codec peers/qualities/ABR, audio output,
boot/shutdown video and diagnostic retrieval in the one integrated run.
Distribution: optional codec enablement is the private integration profile;
product controls do not resolve redistribution/certification questions.
Final lane state: **PARTIAL / source implemented and software validated**,
with full sleep and high-resolution capabilities still platform/physical gates.

## Priority 4 — Battery/charging

Starting state already includes 70/450/650 mA source ceilings, 4.175 V CV,
voltage-limited termination/hold/recharge, watchdog ownership, SOC estimate,
warning/critical policy, and a debounced 3.4 V shutdown floor. Reborn already
saves session, stops audio, closes SQLite and acknowledges the bounded platform
shutdown; the coordinator syncs and requests ordinary init shutdown. These
paths were reviewed and preserved rather than changing proven electrical
policy without new evidence.

Exact source findings:

- Retained Y2 `fgauge_read_current` at `0xc04caa04` and
  `fgauge_read_columb` at `0xc04caa2c` return zero immediately and never fill
  the output pointer (20-byte functions). These are stubs, not zero measured
  current or a coulomb counter. Same functions in the exact MT6582 BSP also
  return STATUS_OK without producing data.
- Stock temperature conversion assumes a 10 kohm NTC, 16.9 kohm pull-up,
  27 kohm pull-down, 1.8 V. Stock public temperature is constant 25 C.
  Existing physical BATON raw10387–10388/570.6mV did not establish NTC wiring.
- Stock charger-current computation subtracts ISENSE/BATSNS and assumes
  68 milliohm. Existing paired read result (-0.081mV mean,1.638mV standard
  deviation with charging disabled) cannot validate resistance, channel
  offset, or net pack-current routing. It is not a calibrated current path.
- Existing raw IIO BATON1/ISENSE/BATSNS and die thermal acquisition already
  provide safe instrumentation. A PMIC die temperature is not pack temperature.

Primary sources freshly fetched:
[MT6582 battery_meter_hal.c](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/power/mt6582/battery_meter_hal.c),
[MT6323 AUXADC implementation](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/power/mt6582/pmic_mt6323.c).
Local copies under `out/feature-completion-battery-research/`, SHA256 respectively
`f0abff44297fa5613cec5e8e74cd0bfe60b67348156e1df6bf36d5b5b453cc0c` and
`196b4605c4d367189f6b7b4a6263d617c0c90dfade649020a4e7139acdf27a98`.
Y2-specific retained evidence:
[ADC result](../hardware-evidence/2026-09-14-m4-adc/result.json),
[stock reconstruction](../knowledge/m4-battery-acquisition.md),
[charger policy provenance](../knowledge/m4-charging.md).

Implementation: truthful estimated-SOC presentation only; no fabricated sensor
conversion or unproven register/voltage changes. Linux targeted validation:
power contract **8 PASS**, SOC **5 PASS**, charger **4 PASS**. Existing broader
power/charger tests remain part of parent integrated validation.

Remaining exact blocker: pack/sense schematic or continuity/resistor evidence,
reference current/temperature calibration and actual charge/discharge topology.
The existing `tools/development/sample-battery-adc.sh` records bounded paired
ADC observations; add its results to the integrated harness, not a separate
flash workflow. Owner equipment is required to turn those voltages into proven
pack current/temperature. Fuel gauge/coulomb support is **not established**,
not proved physically impossible. No new image alone can establish wiring.

Final lane state: conservative charging/low-battery source retained and tested;
measurement additions **BLOCKED_BY_HARDWARE_PROOF**. Full discharge reserve,
charging/hold/recharge and low-voltage graceful shutdown remain physical proof.
