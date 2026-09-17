# GPU-01: accelerated graphics pass; suspend qualification blocked

Owner-deployed source `7c43557a38fac6bbdb0ab5628cffe131ebda6360`, Linux
`6.18.0-y2linux-gpu-01`, root `Y2LINUX-GPU-01`. Pinned SSH confirmed both markers.
No assistant flashing or installed kernel/library replacement occurred.
[Payloads and fallbacks](../../build/y2linux-gpu-01-deployment.md) retain their
original hashes. Host capture date is September 18, 2026 Europe/Berlin; the
unnetworked device RTC calendar is August 2022. Boot IDs and monotonic timing
identify sessions; device calendar timestamps are not presented as host dates.

**GPU #34 remains OPEN. Y2Linux is NOT READY TO BEGIN REBORN.** The bounded
graphics, runtime-PM, thermal and wired-audio results below pass. Deep suspend
does not. Only the observed suspend defects are being corrected for GPU-02;
Reborn, M6 and deferred M5 connection/audio work are not started.

## Rendering and normal operation

The first boot ID is `910f0f1b-4a58-4e6c-9c45-c44ca6f88f70`, taint 0.
Mainline Lima probes GP and both PP cores: **Mali-400 MP2 r1p1**,
GP `0x0b070101`, PP `0x0cd070101`; L2 is 64 KiB, four-way, 64-byte lines.
The standard three MMU initialization handshakes and real page-backed rendering
complete without MMU fault IRQs. No separately readable MMU revision is claimed.

| Capability | Physical observation |
| --- | --- |
| Render/KMS nodes | `/dev/dri/renderD128` Lima; existing Mediatek KMS `/dev/dri/card1` |
| EGL | Mesa Project, **1.4**, OpenGL/OpenGL_ES APIs |
| GLES | Mesa, renderer **Mali400**, **OpenGL ES 2.0 Mesa 24.0.9** |
| Config | EGL config 7, linear XRGB8888 scanout, alpha bits 0; RGBA textures and GLES blending |
| Limits | Maximum texture edge 4096, 16 texture units; full queried extensions in `rendering.txt` |
| Presentation | Native 480×360, reported mode refresh 63 Hz; GBM/kmsro PRIME buffers rendered by Lima and directly scanned by existing KMS |
| Visibility | Owner confirms correct colored patches, textured geometry, translucent panels and animation without visible corruption/tearing |
| Two-minute workload | 3600 frames, 3599 page flips, 120.009 s, **30.00 FPS** |
| Submit through presentation | Median **9.272 ms**, p95 **16.191 ms**, maximum **20.276 ms**; synchronization included, not GPU-only timing |
| Application resources | **3.36%** CPU (one-core equivalent); maximum RSS **13076 KiB** |
| Thermal | CPU **46.8–49.6°C**, PMIC **46.298–48.637°C**, 125 collector samples; no GPU sensor claimed |
| Clocks | GPU 500.5 MHz inherited MMPLL; CPU observed 598–1040 MHz under existing policy |
| Charging | Watchdog pets 127→172 during collection, zero fault/sample/stop errors; configured current is not measured battery power |

The utility performs no CPU presentation copy. Its linear KMS buffers alternate
through normal page-flip events; existing OVL programming reports matching DMA
addresses. Both PP job IRQs advance, all three MMU fault counters remain zero.
Allocation strategy remains standard GEM/shmem/PRIME with unchanged reserved
memory. Large allocations or deliberate exhaustion were not tested.

Initial no-client idle and idle after rendering both show `runtime_status=suspended`,
MFG genpd `off-0`, disabled source/G3D clocks and power bit 4 clear. With a live
static EGL context, snapshots five seconds apart show unchanged GPU active time
and GP/PP IRQ counts, domain off, and one USR1-triggered redraw succeeds.
No permanent-power workaround is used. Static client CPU wakeups are bounded
by the qualification utility; Reborn should wait on events/damage.

Five starts/exits restore KMS; TERM exits cleanly. One KILL returns 137,
runtime-idles, and a new renderer succeeds in the same boot. This proves
application termination recovery, not deliberately injected GPU hardware hangs.
Available-memory snapshots vary with system caching; no claim of an exhaustive
leak/allocation-stress test is made.

Six quiet S16 stereo 44.1-kHz ALSA/CS43131 fixtures play during GPU animation,
all exit successfully with no XRUN output. Owner confirms clean sound in both
channels, no clicks or unexpected dropouts. Original mixer controls are restored.
Evdev records balanced wheel, navigation, volume and Power events. The input
observation window outlasted the animation after the combined harness stopped
early, so strict overlap for every control must be confirmed during GPU-02's
targeted final run; aggregate event counts alone do not prove that overlap.

Two GPU/radio regression scans return **19 and 23 BSS entries**, the latter with
Bluetooth powered on (`functions=0x9`). Core errors, transport errors and
recoveries stay **0**. Both radios return off and both saved preferences stay 0.
The first scan attempt preceded wpa_supplicant interface readiness and returned
ENETDOWN; waiting for its normal control-interface readiness resolves the harness
ordering without a radio change or recovery. Association/WPA2/DHCP/DNS/traffic,
saved-network reconnect, peer pairing/SBC/AVRCP and sustained/full coexistence
remain pending by explicit owner choice.

## Exact suspend failures and correction

1. Installed `y2-suspend` refuses to enter sleep although status says
   `powered=0 functions=0x0`: it matches decimal `functions=0` only. The source
   correction accepts both zero formats and continues to reject powered/live
   functions. Execution tests replay the original refusal and verify correction.
   A reviewed copy under `/run/gpu01/fixed-bin` was used for subsequent checks;
   the installed Y2ROOT file was not overwritten.
2. The first actual deep-sleep attempt loses SSH and does not visibly wake on
   the scheduled RTC alarm or the owner's brief Power tap. After the requested
   USB reconnect, a **new boot** is observed:
   `dc6d812e-78ae-4c21-ba23-55b904d26275`. The original tmpfs continuation log is
   lost; the precise reset/poweroff cause is not established. This is a failed
   same-session test, not a resume success.
3. A bounded `pm_test=core` attempt on the new boot retains logs under
   `/data/gpu01-qualification`. Lima runtime/system callbacks and genpd noirq
   callbacks return 0. CPU3 shutdown then reports **-ETIMEDOUT**, CPU2 removal
   is refused, and CPU3 restart is refused by the driver's fault containment.
   SPM never reaches its deep-entry function (`entries=0`). Status changes
   **0x3f4c→0x3d4c**: hardware cleared CPU3 **bit 9**, while the existing code
   waits for **bit 13**. This is the concrete reproducible blocker; it does not
   establish the unretained first attempt's complete failure chain.

Retained actual Y2 stock disassembly independently confirms CPU1/2/3 masks
**0x800/0x400/0x200**, and `spm_cpusys_can_power_down` tests **0xe00** in both
status copies. The pinned GPL source agrees. The targeted correction updates
power ACK polling, CPU boot's already-powered test and the deep-entry guard.
The previous tests repeated the erroneous ascending masks; corrected fixtures
use independent stock constants, replay the failure and cover stuck ACKs and
each real secondary bit. No power sequence, voltage, clock policy, memory map
or PCM program changes. This CPU-mask defect existed before the GPU changes;
the owner-accepted M4 baseline is retained with this newly observed residual.

After the failed staged test the system remains SSH-accessible on CPUs 0–2,
SPM `broken=1`, taint 0, GPU suspended, radios off with zero errors/recoveries.
`pm_test=none`, automatic charge policy and normal backlight are restored; no
test RTC alarm remains. Further hotplug/deep-sleep attempts are stopped on
this faulted boot. Normal charging/watchdog service continues. Root/data remain
mounted and the recovery boot's checks report both filesystems clean; this
does not substitute for same-session resume or wider storage qualification.

## Remaining targeted qualification

Deploy the corrected integrated GPU-02 manually, then confirm its source/markers,
CPU3/2/1 power-off and re-entry through the bounded core test before actual sleep.
Verify static-context deep RTC/Power wake with identical boot ID, redraw using
the retained EGL context, then no-client sleep and a fresh renderer. Confirm
panel/backlight, strict input overlap, the existing offline-charge display and
post-resume audio/radio/storage/power state. Do not repeat graphics discovery,
the full thermal run or the already passed M5 startup foundations.

Ordinary deep-resume EGL preservation/recreation requirements remain unqualified.
GPU-02 host validation or a temporary helper test cannot close this milestone.

## Receipts

`rendering.txt`, `load-telemetry.csv`, `runtime-static.txt`, `recovery.txt`,
`audio.txt`, `input-events.txt`, `input-counts.json`, `radio-result.json` and
`owner-observations.txt` support the normal-operation results. `suspend-original.txt`,
`post-reconnect-new-boot.txt`, `pm-callbacks-and-cpu-failure.txt`, `final-state.txt`,
`stock-cpu-contract.txt`, `stock-cpu-source.json` and before/after regression logs
support the focused corrections. Public addresses are redacted; raw captures
remain in ignored `evidence-private/20260918-gpu01`. `SHA256SUMS` covers these
public receipts. No owner credentials, firmware or calibration data are added.
