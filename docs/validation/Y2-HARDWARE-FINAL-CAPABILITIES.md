# Y2 Hardware Final capability ledger

**Paused at owner request,2026-09-27.** The [handoff](Y2-HARDWARE-FINAL-HANDOFF.md)
supersedes provisional progress below and records restoration, completed fixes,
owner-confirmed volume repair and all outstanding validation. No final image.

ACTIVE/provisional,2026-09-27. Status describes the explicit measured scope in
each row, not blanket final-platform acceptance. Only owner physical evidence
can promote final-source changes. MB/s is decimal bytes; Mb/s decimal bits.
See [campaign](Y2-HARDWARE-FINAL.md) and [qualification](Y2-HARDWARE-FINAL-QUALIFICATION.md).

| Capability | Status | Evidence / precise limit |
| --- | --- | --- |
| eMMC width/clock/timing | QUALIFIED_WITH_LIMIT | 8-bit, actual49,999,942Hz high-speed SDR3.3V; bounded13/25/50 sweep and direct/durable readback pass. Repeats/library/endurance pending. |
| eMMC measured read/write | QUALIFIED_WITH_LIMIT | First50MHz 1MiB direct read33.08/write26.44MB/s; workload/cache/CPU limits retained. Not an endurance maximum. |
| SD width/clock/mode/read/write | QUALIFIED_WITH_LIMIT | One known-good SD128:4-bit SD high-speed3.3V; first50MHz read19.19/write17.95MB/s. Multi-card/removal lifecycle remains open. |
| USB PIO/DMA | FAILED | Device ECM works; DMA controller exists but DMA IRQ count stays6 across large transfers. Source aligns ECM buffers2mod4 while Inventra requires4-byte alignment; final fix in investigation. |
| USB raw TCP/SFTP/reconnect | QUALIFIED_WITH_LIMIT | Hardware02 TCP~38Mb/s into unit/~46Mb/s out, RX retransmissions; current separated SFTP and reconnect pending. Historical PIO results are comparison only. |
| CPU frequency/OPPs/voltage | IMPLEMENTED_PHYSICAL_PENDING | Baseline598/747.5/1040MHz1.15V. Guarded exact-bin stock1196MHz1.20V/1300MHz1.25V opt-in under implementation; default1040MHz cap. No qualified high-OPP claim. |
| CPU core policy | QUALIFIED_WITH_LIMIT | Four online default; earlier bounded1–4-core cycles passed. Hardware02 playback/hotplug repeat and measured power benefit pending; no automatic hotplug. |
| Local timer/highres/NO_HZ | QUALIFIED_WITH_LIMIT | Four arch_sys_timer/PPI29 clockevents, arch_sys_counter13MHz; hres_active/nohz1 and idle_sleeps observed. Continuity, load, hotplug and effective idle deltas retained separately. |
| Cpuidle states/residency | BLOCKED_BY_HARDWARE_EVIDENCE | WFI only. Exact stock slidle/dpidle found; deeper state ownership/eligibility and power/latency proof absent. Do not use suspend PCM as runtime idle. |
| Suspend Power wake/cycles | FAILED | Prior persistent devices/platform/processors return but USB overflow and radio restore fail; no same-boot deep wake acceptance. Current staged test pending. |
| Suspend RTC wake | BLOCKED_BY_HARDWARE_EVIDENCE | Requires restored deep suspend then same-boot alarm/wake-source/drift receipt. |
| GPU clock/runtime PM | QUALIFIED_WITH_LIMIT | Inherited stock branch500.5MHz, Mali400MP2; current renderer active, historical screen-off runtime suspend. No evidence-backed higher clock; new endurance/resume pending. |
| Charging behavior/limits/thermal | QUALIFIED_WITH_LIMIT | SDP500mA allocation/configured450mA,4.175V target, real voltage/state/die temperatures. No USB meter; load energy balance/current/full behavior unmeasured. |
| Battery current | BLOCKED_BY_HARDWARE_EVIDENCE | Stock FG stubs; ISENSE-BATSNS topology/resistor/calibration unproved. No net-current property. |
| Battery SOC/percentage | BLOCKED_BY_HARDWARE_EVIDENCE | Hardware coulomb integration unestablished, calibrated rest/discharge/charge curves absent. No invented percentage or precision. |
| Battery temperature | BLOCKED_BY_HARDWARE_EVIDENCE | BATON stable~10387; NTC versus fixed detection resistor unresolved. Die temperatures never substitute. |
| Low-battery warning/critical/shutdown | BLOCKED_BY_HARDWARE_EVIDENCE | Mechanism implemented but thresholds disabled; measured safe reserve/load sag and shutdown exercise required. |
| Wi-Fi throughput/reconnect/power save | QUALIFIED_WITH_LIMIT | WPA2/DHCP/DNS/NTP/TCP baseline works. Current throughput/coexistence measured separately. Observation timeout metrics bug identified; AP-loss/reboot/PS/endurance pending. |
| Bluetooth SBC | QUALIFIED_WITH_LIMIT | Hardware02 AirPodsPro2: negotiated stereo44.1k S16, owner clean listening and stem Play/Pause. Volume-key restart bug requires retest; longer endurance/fresh bond pending. |
| Bluetooth SBC XQ | IMPLEMENTED_PHYSICAL_PENDING | libsbc/BlueALSA support exists; no XQ bitpool/peer/coexistence qualification, normal HQ fallback retained. |
| Bluetooth AVRCP | QUALIFIED_WITH_LIMIT | Owner confirms Play/Pause; captured MPRIS Play/Pause from BlueZ. Next/Previous/metadata need distinct observations. |
| Bluetooth AAC | QUALIFIED_WITH_LIMIT | Temporary owner-private ARM FDK/BlueALSA bundle negotiates AAC,S16 stereo48k; owner clean listening. CPU/coexistence/long endurance and final image remain separate gates. |
| Bluetooth aptX/aptX HD | IMPLEMENTED_PHYSICAL_PENDING | Pinned LGPL libfreeaptx ARM builds; available AirPods do not establish peer support. No Adaptive/Lossless claim; public product clearance unestablished. |
| Bluetooth LDAC | IMPLEMENTED_PHYSICAL_PENDING | Pinned encoder+ABR ARM builds. No suitable peer for330/660/990/ABR; certification/public distribution unestablished. No990default. |
| Bluetooth Auto | IMPLEMENTED_PHYSICAL_PENDING | Existing typed policy remains gated by actual compiled/peer/qualification evidence; preference is never active codec. SBC fallback retained. |
| Wi-Fi/BT coexistence | IMPLEMENTED_PHYSICAL_PENDING | SBC/AAC samples and checked TCP scheduled with exact codec/timestamps; scans/reconnect/other-codec peers and long duration outstanding. |
| Wired S16/44.1 | QUALIFIED_WITH_LIMIT | Prior owner-confirmed native baseline; current final application regressions/listening remain separate. |
| Wired S16/48 | IMPLEMENTED_PHYSICAL_PENDING | Prior short direct48k clean; normal product profile44.1 only until longer product/screen-off/switching evidence. |
| Wired S24/S32/preserved24-bit | BLOCKED_BY_HARDWARE_EVIDENCE | Exact MT6582 DL1 wide fetch/interconnect packing not established; donor hd_reg=-1. CS43131/32-bit I2S slots are insufficient. |
| Wired88.2/96kHz | BLOCKED_BY_HARDWARE_EVIDENCE | Complete DL1 format/AFE clock-family path and measured clocks missing; fallback resampling retained. |
| USB host/HID/storage/USB Audio | BLOCKED_BY_HARDWARE_EVIDENCE | Connector ID routing, VBUS source switch, current limit/protection unestablished. Need board tracing/electrical measurement; never energize guessed VBUS. |
| CS43131 additional features | QUALIFIED_WITH_LIMIT | Existing DAC volume/filter controls available; load/IRQ/jack topology not qualified. No automatic high gain or impedance policy. |
| RTC read/write/tick/NTP | QUALIFIED_WITH_LIMIT | Receipt05 UTC write/read/tick passes and NTP sync policy enabled. Reboot/full-power-off retention not yet proved. |
| RTC alarm wake | BLOCKED_BY_HARDWARE_EVIDENCE | Alarm register availability is not wake proof; depends on same-boot deep resume. |
| Display/memory | QUALIFIED_WITH_LIMIT | Render/display/input observed; HIGHMEM/reservations retained, PSS available. Screen-off/GPU/load/leak pressure/endurance still bounded tests. |
| OTA update/rollback | IMPLEMENTED_PHYSICAL_PENDING | Signed root/rescue/readback/health/rollback code; destructive apply requires explicit owner approval after final stable candidate. No bootloader corruption. |
| Recovery/reboot/power cycles | IMPLEMENTED_PHYSICAL_PENDING | Hardware02 boots and pinned SSH works; final repeated cycles and controlled userspace-failure receipts pending. |
| Endurance | IMPLEMENTED_PHYSICAL_PENDING | Short workload measurements only; no8h wired/BT or full combined endurance completion claimed. |
