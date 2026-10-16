# Beta 8: FM radio — gap audit evidence and phased plan (2026-10-16)

**Status: PROPOSED. Nothing is built, flashed or powered.** The owner agrees to the plan and answers the open decisions
(§5) before any phase starts; the first tuner power-up (phase 2) needs its own explicit go. Audit row:
[BETA-8-FM](roadmap-gap-audit.md#beta-8-fm-radio-milestone-gap-audit--2026-10-16).

## 1. Evidence used

| Source | Origin / date | What it shows | Limits |
| --- | --- | --- | --- |
| `../Y2Player/docs/FM_RADIO_FEASIBILITY.md` | Owner's Y2, stock Android + Y2Player 2.5, 2026-08-01…03 | Tuner opens (chip id 0x6627), tunes, plays audibly on headphones through the HAL's indirect path. Reception: noise floor −100…−107 dBm, no real station, RDS never decoded. | Indoors only; 0.5 MHz sweep at 120 ms (too coarse, too fast); §9 controls never run. |
| Stock kernel `out/rea/fm-kernel.elf` (v3.2.0 FM package, symbols intact) | Static, this audit | Complete vendor stack: `wmt_func_fm_on/off`, `mtk_wcn_consys_hw_vcn28_ctrl` → `hwPowerOn(LDO_VCN28)`, `fm_link_*` over STP, `mt6627_*` chip ops (`PowerUp` 0x8b8 bytes, `SetFreq` 0x3d8), RDS, desense hooks (`LCD_FM_Desense`, `DSI_FM_Desense`). `wmt_plat_i2s_ctrl` is a log line returning 0: the FM I2S is SoC-internal, no pin mux. | Static only. |
| Stock HAL `libaudio.primary.default.so` (757 172 B, SHA-256 `409430ce…cc4304`, from the v3.2.0 FM `system.img`), REA/Ghidra 12.1.2 | Static, this audit | `WCNChipController::InitAudioFMInfo` asserts the path is **not** analog. "Direct" mode = digital hardware loop I00/I01 (FM I2S in) → O15/O16 → HW gain 1 → I12/I13 → **O00/O01**, the same outputs our AFE driver sends to the CS43131. "Indirect" mode = the FM I2S captured into memory (`AUDIO_DEVICE_IN_FM` 0x81000000 → memif type 5) through the I2S ASRC when rates differ (asserts AFE I2S-in slave, FM master), then mixed by software. | Static; the reason the stock direct mode was silent on the Y2 is not known. |
| Donor `donorSource/drivers/misc/mediatek-consys/fmradio` (artificery `53fb57bb`, 15 189 lines) | Source | Vendor driver forward-ported to 6.12: FM on = (co-clock) VCN28 on + WMT opcode 6 function 1; FM traffic on STP channel 1 (`FM_TASK_INDX`); patch/coeff table v1…v5; default audio config I2S, CONN pad, FM master, 32 kHz. | Not run on the Y2 under Linux; Android-style ioctl ABI. |
| Owner firmware packages (`y2_v3.2.0_FM-20260813`, `OriginalFirmware`) | Static, this audit | Both carry identical FM files; only v1 is real (v2 coeff 2 B, the rest empty). See §3. | — |
| Y2 over USB SSH, **read only** | Beta 7.6 (`6.18.56-y2linux-1.0.0-beta.7.6`), 2026-10-16 | Connectivity `activated=1 powered=1 functions=0x8` (Wi-Fi), `chip=6582 hvr=8a01 fvr=8a00`, 0 errors/retries. Rails: vcn18 on, **vcn28 off (0 users)**, vcn33-bt off, vcn33-wifi on. No `/dev/fm`, no FM firmware on Y2DATA. ALSA: one playback PCM only. Input "Y2Audio Headphone" advertises SW_HEADPHONE_INSERT but nothing drives it. 858 MiB available. | No register writes, no tuner power. |

## 2. Gaps (what Y2Linux lacks today)

1. **Kernel transport** (`kernel/platform/connectivity/`): `y2_conn_get/put` and `y2_wmt_function` accept only BT/Wi-Fi;
   `y2_stp_send`/`deliver` only WMT and BT channels; `power_on_once` forces VCN28 off ("co-clock") and no path turns it on;
   recovery does not notify an FM consumer.
2. **Tuner driver:** none. Needs the MT6627 packet layer (FM opcodes on STP 1), patch/coeff download, power-up sequence,
   tune/seek/scan, RSSI/CQI, mute/volume, I2S on, later RDS. The register sequence comes from the stock ELF; the donor is the
   readable reference.
3. **Audio:** `kernel/audio/mt6582-afe.c` is playback-only ("No FM, capture"). Needs AFE I2S-in (slave), I2S ASRC
   32 kHz → 44.1/48 kHz, interconnect and a capture memif, exposed as an ALSA capture PCM; optionally the hardware loop to O00/O01.
4. **Firmware intake:** `tools/platform/y2_platform/radiofw.py` knows five Wi-Fi/BT files; FM needs three more (§3). Players that
   already installed firmware deleted their `system.img`; older roots never had FM files, so the recovery copy cannot supply them.
5. **Platform API:** no FM service, scan cache, activity lease or sleep policy for FM. Connectivity `suspend()` refuses while any
   function is on, so FM must stop (or sleep must wait) before full suspend.
6. **Reborn:** no FM source; internet radio (`reborn_platform::radio`, screens Radio/RadioStations) is the neighbour.
7. **Antenna/jack:** the headphone cable is the aerial, and wired-headphone detection is a known open gap (DTS: "EINT16/jack is a later test").
8. **Reception on the owner's unit is unproven** — the dominant risk. Nothing above helps if the RF input is dead.

## 3. Firmware (owner's `system.img`, never in images)

| File (stock path) | Size | SHA-256 | MD5 (matches feasibility §9.5) |
| --- | --- | --- | --- |
| `/etc/firmware/mt6627/mt6627_fm_v1_patch.bin` | 1028 | `ba35c74d26660729caf83b41a567e700f633b6c99384dfa6cbb3c17ceb767e26` | `7fee6d2d…0d19` |
| `/etc/firmware/mt6627/mt6627_fm_v1_coeff.bin` | 1092 | `d32f4a16b35f950ed1a57f2ae88f77ac4b5674d5c4210b65f6fe7912901fd8db` | `8ed159e3…35f5` |
| `/etc/fmr/mt6627_fm_cust.cfg` | 345 | `d6457a1ccf5171cabf57c9988c88e9900a96e444a36bef8491c063fdbd1fd33c` | — |

The cfg holds only thresholds (RSSI −296/−296, desense −240, SMG 16421), 50 µs de-emphasis and a 26 MHz oscillator; these can
also be compiled-in defaults, which would drop it from the intake (decision 7).

## 4. Phased plan

Each phase ends with targeted validation (AGENTS.md); the release candidate gets the full boundary.

**Phase 1 — firmware intake (host + platform, no tuner power).** `radiofw.py` accepts the FM files as an *optional* group with
reviewed sizes/hashes: Wi-Fi/BT readiness never depends on them; a `system.img` dropped on an already provisioned player adds
only the FM files. State in `/run/y2/radio-firmware.json` gains `fm: present|missing`. Host tests with synthetic images.
Check issue #39 (oversized `system.img` on Y2DATA) on the same path.

**Phase 2 — kernel FM function and tuner bring-up (first hardware power-up; separate owner go).**
- Connectivity: `Y2_CONN_FM = 1` in get/put; VCN28 in software mode on for FM only (stock `hwPowerOn(LDO_VCN28)`), off after;
  WMT opcode 6 function 1; STP channel 1 send/deliver to an FM receiver; whole-chip recovery stops FM cleanly.
- New `kernel/platform/connectivity/fm.c`: the MT6627 subset needed for receive — packet layer, patch/coeff via
  `request_firmware("mediatek/mt6582/mt6627_fm_v1_*.bin")`, power-up (`pwrup_clock_on`, digital init, fine tune), `SetFreq`
  with channel parameters, mute/volume, RSSI/PAMD/full CQI, soft-mute tune, hardware seek/scan, mono/stereo, I2S on (master, 32 kHz).
  No TX, no Android ioctl ABI.
- Interface: a standard **V4L2 radio device** (`/dev/radio0`: frequency, seek, signal/stereo in `G_TUNER`, RDS reads later)
  plus a root-only debugfs dump of CQI for the lab.
- **Experiment (owner presses buttons; image via the usual owner flow):** Wi-Fi off; power FM; read chip/ROM version and patch
  result; tune a station the owner confirms on another receiver; repeated RSSI/CQI with no cable, cable A, cable B; full band
  sweep at 100 kHz (50 kHz if decision 2 says so) with 1–2 s settle and median of several reads. This is feasibility §9.1/9.2/9.4
  done under Linux, where we control every register.
- **Gate:** a known station clearly above the floor (several dB, stable) → continue. Otherwise stop: FM stays hidden, and the
  owner decides on the physical checks of feasibility §9.6 (jack-to-MT6627 path, nearby low-power transmitter).

**Phase 3 — audio path.** AFE: I2S-in slave + ASRC to the output rate, interconnect to a capture memif, ALSA capture PCM
"Y2 FM Capture" (S16, stereo, 44.1/48 kHz). Reborn reads it into the existing sink, so volume, the wired path and (decision 5)
Bluetooth behave like any other source. Bit layouts from the MT6582 manual and the stock HAL's interconnect numbering above;
confirmed read-only against stock register dumps where possible. A zero-CPU hardware loop (I00/I01 → O00/O01, stock "direct") is a
later power optimisation, wired only.

**Phase 4 — platform API.** One FM owner in the platform: power/tune/seek/scan/signal/RDS, firmware state, scan cache on
Y2DATA, activity lease while FM plays (sleep with the screen off like music), clean stop before full suspend and on Power sleep,
coexistence with Wi-Fi/BT (shared CONSYS; stock has display/CPU desense hooks — replicate only if interference shows).

**Phase 5 — Reborn UI.** Radio section gains "FM" beside internet radio, shown only after a scan finds a station (owner
decision 2026-10-15). FM screen: frequency, signal, stereo (and RDS name/text if decision 3). Wheel tunes, Next/Previous
seek, hold saves (decision 4), first-run scan builds the list. A hint that wired headphones are the aerial. Skins get the new screen.

**Phase 6 — qualification and Beta 8.** FM on/off loops, seek/scan, long play on battery (drain per hour, CPU/temperature),
sleep/wake around FM, Wi-Fi/BT on together, STP recovery during FM, firmware-absent path, OTA install; audit row and report.

## 5. Open owner decisions

1. **Authorisation and gate:** agree to phases 1–2 and the phase 2 experiment; accept the reception gate (stop and keep FM hidden if no real station is received).
2. **Band and step:** 87.5–108 MHz (stock Y2 config); scan/tune step 100 kHz, or 50 kHz (some German transmitters sit on a 50 kHz raster); wheel step.
3. **RDS** (station name, radio text) in Beta 8 or later.
4. **Presets:** shared Favorites with internet stations, or a separate FM preset list.
5. **Bluetooth output:** allow FM over Bluetooth (capture path makes it possible; a wired cable is still needed as aerial), or wired only like stock.
6. **Sleep while FM plays:** keep playing with the screen off (activity lease) and stop on Power sleep — recommended.
7. **Firmware re-intake** for already provisioned players: ask the owner to copy `system.img` again when FM is first opened; keep or compile in the cfg.
8. **Release shape:** ship Beta 8 with FM hidden on players that find no station even if the owner's unit cannot receive, or hold FM until one unit is proven.

Recommendations where a choice is technical rather than the owner's: V4L2 radio interface; capture path first, hardware loop later.
