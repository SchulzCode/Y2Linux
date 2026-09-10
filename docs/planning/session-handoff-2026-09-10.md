# Session handoff — 2026-09-10

Canonical repo: `/home/luca/Dokumente/Code/Y2Linux`, branch **main**,
remote `https://github.com/SchulzCode/Y2Linux.git`. Read this first, then the
[current audio result](../knowledge/m3-audio-01-live-result.md) and
[roadmap current audit](roadmap-gap-audit.md#current-session-close-audit--2026-09-10).
The session closes with documentation/evidence/Git only. No new feature, M4 work,
build, flash or device change. Last tested headphone state is off.

## Current physical system

`root@10.42.0.1`, hostname `y2linux`, kernel `6.18.0-y2linux-m3audio02`.
Linux6.18 boots Buildroot2025.02.17 on writable removable SD Y2ROOT; initial
USB Ethernet/key SSH and CDC ACM work, including return after owner restart.
Four Cortex-A7 CPUs, visible display, all navigation/volume/brief Power keys,
click-wheel and PMIC/PWRAP/regulator/I2C/GPIO/EINT foundations work. Internal
eMMC host11230000 stays disabled/unmounted; root is SD host11240000.

DEV-02 actual MemTotal **954660 KiB**; completed **256 MiB allocator RAM test passed**.
AUDIO-01 measured **954384 KiB** after the larger audio kernel. Physical bank is
992 MiB, reservations leave about951 MiB before kernel/page metadata; MemTotal is
smaller. AUDIO-02 /proc/meminfo was not separately retained; do not invent a new
measurement. Source memory layout/reservations remain unchanged.

**USB reconnect #27 remains unresolved.** Unplug/replug does not restore the
link without restarting. Initial ACM/Ethernet/SSH success remains valid. Do not
repeat reconnect tests as part of tomorrow's small audio qualification.

SD base build-id remains `Y2LINUX-DEV-01`; permanent ALSA tools/config/WAVs were
installed additively from the built M3 rootfs, not a full rootfs replacement.
Use ordinary `aplay`, `amixer`, `speaker-test`, `y2-audio-collect` and
`/usr/share/y2linux/audio/headphone-{44100,48000}.wav`. No temporary restoration
or SD replacement is needed. [Reproducible installation](../knowledge/audio-tools-sd.md).

## Audio and deployed artifact

ALSA card `Y2Audio`, PCM0 `Y2 Headphone Playback`, endpoint
`hw:CARD=Y2Audio,DEV=0`: DL1 -> I05/I06 -> O00/O01 -> second I2S CON3 -> upstream
CS43131 -> headphones. VGP2=1.8V with ordered GPIO20/18/15; speakerGPIO8 off.

| PCM | Status |
| --- | --- |
| S16_LE stereo 44.1 kHz | Physically clean; owner "perfect, no clicking!" |
| S16_LE stereo 48 kHz | Implemented, physical test pending |
| Native 24/32-bit and higher rates | Not implemented; later enhancement |

32-bit I2S slots carry 16-bit samples. Future Y2PlayerNative can use standard
ALSA for CD-quality audio, but its implementation has not started; higher-res
sources currently require conversion. No high-resolution development now.

`137b3f2` fixes positive ALSA constraint success skipping substream attachment
and therefore period notifications. AUDIO-01 could replay queued DMA contents
without ALSA refills; audible sound with clicking was not correct PCM.
Production startup regression covers0/+1/error returns. **Fix is already built,
owner-deployed and tested in AUDIO-02**:3.13s run, 130IRQs, advancing ALSA hw_ptr,
no reported underrun/new audio error, clean owner listening. No redeployment due.

- Integrated implementation: `aa3c949`; fix: `137b3f2`.
- Deployed candidate commit: `8f93e44918518bfc3246aad1867dd0701f07e056`.
- Persistent tools: `9c90141`; clean-result evidence: `7e0413d`, `7d9fed1`.
- BOOTIMG: `out/y2linux-m3-audio-02/BOOTIMG.img`, **4816896bytes**.
- SHA256: `b9dbf6b528e4afbee2a5ffb2fc65df7f6a076564e92960332e24cee3809b5842`.
- [Retained packaging metadata](../build/evidence/y2linux-m3-audio-02/): its
  untested/temporary-tool wording is the immutable predeployment snapshot.
- Existing full rootfs alternative: `out/y2linux-m3-audio-01/rootfs.ext4`,
  SHA256 `742bf915ec74cd2f5bc8c5a8f5be40eff8d2c5d5b1b12fb881c84c59e9bd4610`.
  It is **not required for tomorrow**. No new rootfs image was built for AUDIO-02.

## Exact first task tomorrow

Finish the **small remaining M3 qualification on the existing kernel**:

1. Verify SSH kernel/card identity. The period-notification deployment/test item
   is already done; do not flash it again.
2. With owner headphone readiness, run the short quiet S16/48 kHz WAV.
3. Verify explicit left/right separation (current fixture is L440Hz/R660Hz).
4. Short stop/restart/repeated playback; monitor pointer/IRQ progress, duration,
   underruns, DAC state/power/dmesg and owner reports of distortion/pops/resets.
5. If those pass, refresh the milestone audit, record separate speaker/jack/
   precision-clock/high-resolution gaps, then progress to M4. Do not declare
   complete from the existing single clean run alone.

First PC command:

```sh
ssh -i /home/luca/.ssh/y2linux_ed25519 root@10.42.0.1
```

First Y2 commands:

```sh
uname -r
cat /proc/asound/cards
cat /proc/asound/pcm
aplay -l
amixer -c Y2Audio sget Headphone
y2-audio-collect
```

For playback use Master 207=-24dB, PCM Ch Select0 and the faded three-second
-30dBFS fixtures. Adapt the retained
[bounded playback script](../hardware-evidence/2026-09-10-m3-audio-02-playback/play-44100.sh)
to48 kHz; preserve its6s watchdog, cleanup and evidence capture. Older SD has no
`timeout` utility. Stop on loud noise/reset/errors; Headphone off afterward.
No arbitrary devmem/i2c/PMIC writes. Do not rediscover AFE/CON3/CS43131/VGP2.

## Milestones and repository hygiene

M1 COMPLETE; M2 core qualified for progression (#22–26 CLOSED), #27/#28 OPEN for
reconnect/final core qualification. M3#29 ACTIVE / NEAR COMPLETION. M4#30 and
M5#31 NOT STARTED. M4 is next: battery/charger/thermal/cpufreq/cpuidle/suspend/
wake/reboot/poweroff. M5 Wi-Fi/Bluetooth, GPU/lima and final reusable-platform
qualification follow before Y2PlayerNative. M0 residual provenance/recovery gaps
remain separate; no routine re-inventory is needed.

Source/config/DT/overlays/build scripts/tests are tracked. Small build metadata,
reviewed logs and hashes are tracked under docs; actual kernel/Buildroot images,
objects/toolchains and caches remain ignored in `out/` and `.cache/`. Immutable
`donorSource/` and private captures in `evidence-private/` stay ignored. No private
SSH material is committed; the private key path above is an invocation reference.
No important untracked workspace notes remain. Historical raw/private captures
have dated canonical findings/identity/limits in the core and audio reports.

Owner alone performs BOOTIMG-only SPFT flashes; never PRELOADER/LK/partitions/
NVRAM/calibration/internal Android data. Preserve Git identity Luca and the
existing authenticated SchulzCode account; no co-author/model attribution.

## Closeout checks

PASS: current documentation links resolve; JSON manifests parse; candidate
AUDIO-01/02 artifacts still match their original SHA256SUMS and retained copies.
All88 files listed by the four core/current-audio hardware-evidence checksum
manifests pass. Reviewed additions contain no private keys/tokens or oversized
outputs. Source/config/build/test trees are unchanged in this closeout, so no
kernel/Buildroot rebuild or repeated regression/hardware test was needed.
GitHub#22–26 remain closed;#27–32 remain open. Published#27–31 current bodies
were read back and verified: [issue snapshot](session-issues-2026-09-10.json).
No active SSH command is left from this turn; the device was not accessed or
changed during closeout. Ignored build/cache/private/donor trees remain local.
