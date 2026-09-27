# Y2 Hardware Final capability ledger

Built implementation/default inventory, Linux `067003f` / Reborn `5b5d23b`, 2026-09-27.
Every flag describes this candidate, not historical Hardware02 evidence.
`implemented`, `enabled`, `qualified` and `experimental` are independent.
Runtime `y2-platform capabilities` resolves radio and codec enablement from the
installed inventory and explicit owner policy; active codec/PCM is observed
separately. Actual timer and storage state is in `y2-platform status`.

| Capability | Implemented | Default enabled | Qualified | Experimental | Reason / limit |
| --- | --- | --- | --- | --- | --- |
| `telemetry` | true | true | false | false | physical qualification of this exact candidate pending |
| `health` | true | true | false | false | physical qualification of this exact candidate pending |
| `storage` | true | true | false | false | physical qualification of this exact candidate pending |
| `wifi` | true | false | false | false | physical qualification of this exact candidate pending |
| `bluetooth` | true | false | false | false | physical qualification of this exact candidate pending |
| `usb_device` | true | true | false | false | physical qualification of this exact candidate pending |
| `usb_host` | true | false | false | true | VBUS physical validation pending; host role refused before session or VBUS write |
| `audio` | true | true | false | false | S16 44.1/48 enabled independently of qualification; high source rates convert with explicit fallback |
| `power_observation` | true | true | false | false | SOC estimate available; pack current/temperature remain unavailable unless real BAT0 properties exist |
| `low_battery_shutdown` | true | true | false | false | conservative stock-boundary voltage floor; provisional SOC only warns |
| `deep_suspend` | true | false | false | false | same_boot_resume_unqualified |
| `cpuidle` | true | true | false | false | physical qualification of this exact candidate pending |
| `system_watchdog` | true | false | false | false | AP_watchdog_driver_available_recovery_policy_unqualified |
| `ota` | true | true | false | false | physical qualification of this exact candidate pending |
| `automatic_bootimg_update` | false | false | false | false | single_slot_rescue_can_be_lost |
| `shutdown` | true | true | false | false | physical qualification of this exact candidate pending |
| `emmc` | true | true | false | false | physical qualification pending |
| `sd` | true | true | false | false | physical qualification pending |
| `usb_dma` | true | true | false | false | physical qualification pending |
| `usb_ncm` | true | false | false | true | module compiled; ECM remains recovery default; NCM activation and FIFO integration pending |
| `local_timer` | true | true | false | false | physical qualification pending |
| `high_resolution_timers` | true | true | false | false | physical qualification pending |
| `no_hz_idle` | true | true | false | false | physical qualification pending |
| `cpufreq` | true | true | false | false | physical qualification pending |
| `dvfs` | true | true | false | true | stock-bin voltage control implemented; high stock OPPs default gated |
| `workload_qos` | true | true | false | false | physical qualification pending |
| `cpu_hotplug` | true | true | false | false | physical qualification pending |
| `slow_idle` | true | false | false | true | stock slow-idle wrapper implemented; single-CPU clock preflight and physical tuning pending |
| `cpu_power_down_idle` | false | false | false | true | runtime dormant/CIRQ/GPT deadline integration remains a technical gap; suspend PCM is not runtime idle |
| `charging` | true | true | false | false | physical qualification pending |
| `battery_soc` | true | true | false | false | physical qualification pending |
| `rtc_time` | true | true | false | false | physical qualification pending |
| `rtc_alarm` | true | false | false | true | alarm software implemented; same-boot suspend wake pending |
| `wifi_power_save` | true | false | false | true | off/standard/automatic implemented; automatic defaults conservatively off |
| `radio_coexistence` | true | true | false | false | instrumentation and policy hooks implemented; no final bitrate overrides |
| `bluetooth_sbc` | true | true | false | false | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `bluetooth_sbc_xq` | true | false | false | true | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `bluetooth_aac` | true | false | false | true | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `bluetooth_aptx` | true | false | false | true | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `bluetooth_aptx_hd` | true | false | false | true | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `bluetooth_ldac` | true | false | false | true | runtime enablement uses compiled inventory and explicit Experimental gate; physical qualification pending |
| `wired_48` | true | true | false | false | source-backed native S16 path enabled; full candidate qualification pending |
| `audio_rate_conversion` | true | true | false | false | physical qualification pending |
| `wired_s32` | false | false | false | true | exact MT6582 DL1 wide-fetch/high-rate register path not defined in inspected pinned sources; source-backed conversion fallback implemented |
| `wired_native_88200` | false | false | false | true | exact MT6582 DL1 wide-fetch/high-rate register path not defined in inspected pinned sources; source-backed conversion fallback implemented |
| `wired_native_96000` | false | false | false | true | exact MT6582 DL1 wide-fetch/high-rate register path not defined in inspected pinned sources; source-backed conversion fallback implemented |
| `cs43131_controls` | true | true | false | false | physical qualification pending |
| `gpu_runtime_pm` | true | true | false | false | physical qualification pending |
| `display_off` | true | true | false | false | physical qualification pending |
| `memory_telemetry` | true | true | false | false | physical qualification pending |
| `diagnostics` | true | true | false | false | physical qualification pending |

## Defaults, fallbacks and tuning

| Group | Default and fallback | Physical qualification / tuning |
| --- | --- | --- |
| eMMC / SD | Standard SDR automatic maximum 50 MHz; faults lower to 25/13; 8/4-bit. No DDR/UHS/HS200. CID/geometry identity accepts missing UUID. | Matched scratch readback/durable tests, multi-card/removal, maximum stable clock, fault counters, endurance. |
| USB | Inventra DMA; `y2.usb_dma=off` PIO. ECM/ACM USB-only key-auth SFTP; reserves and durable publication retained. | DMA delivered throughput versus programmed counters, RX/TX, reconnect/suspend, retransmits and ENOSPC publication. |
| Host / NCM | Host stack/classes compiled, role refused before VBUS. NCM/dependency and host audio/dependencies carried by BOOTIMG within unchanged size limits; default built-in ECM remains. | Exact VBUS topology before host activation. NCM needs a reviewed gadget-selection/FIFO integration; compilation alone is not active NCM. |
| CPU / timer | schedutil, 598/747.5/1040 MHz table; bin-checked 1196/1300 gated; voltage floor 1.15 V. Per-open expiring QoS, interaction 250 ms. GPT6/PPI29 with legacy fallback. | OPP transitions/readback, scheduler/QoS, thermal limits, all-core hotplug and timer continuity/broadcast/highres/tickless residency. Floors/durations provisional. |
| Idle / suspend | WFI; compiled SLIDLE defaults off, single CPU/gated peripherals/clock rollback. Deep suspend explicit existing owner qualification path and pm_test. | SLIDLE entry/abort counters/residency/wake. Suspend staged failure snapshot, CPU/USB/radio/DRM/ALSA restoration, Power and RTC same-boot wake. |
| Charging | Source-aware BC1.1, stock protections, 4.175 V; supported 70/450/650 mA configurable ceiling. No arbitrary charger voltage/current. | Real external current, source class/USB enumeration, pack ceiling, thermal behavior, termination/recharge/watchdog. |
| Battery | Estimated table in `/etc/y2linux/battery-profile.json`; private owner override in `/data/system/platform`. Hardware/hybrid takes precedence when real. Sensors absent -> null/state-only. | Rest/charge/load/discharge calibration replaces provisional curve, qualified current/resistor/NTC only if actual hardware path exists. |
| Low battery | Provisional SOC warns; independent 3.4 V floor after five samples, ten-second grace; schema-1 owner policy retained. | Reserve, load-sag immunity, warning/countdown/recovery, SOC trust and final thresholds. No synthetic shutdown demonstration here. |
| Wi-Fi / coexistence | Retained checksum repair. Standard power-save callback, automatic conservatively off, off fallback. Traffic and CPU/link hooks, no bitrate override. | Association/DHCP/DNS/NTP/reboot/AP-loss, PS readback/power/latency, isolated throughput and all-codec heavy coexistence. |
| Bluetooth | SBC default. Experimental policy enables compiled private optional codecs and XQ; qualified production Auto separate. LDAC standard/ABR, 990 not mandatory. | Peer/negotiated/active PCM agreement, every codec, AVRCP, reconnect/screen-off/endurance and CPU/radio pressure. Source/license inventory retained. |
| Wired / CS43131 | Native S16 44.1/48; high source rates convert by family then 44.1 fallback. Existing hardware volume/filters/DAPM; no automatic high gain/load work. | Rate switching, low bits, I2S clocks, clicks/volume/screen-off; load sensing needs verified board measurement path. |
| RTC | UTC read/write/persistent floor; NTP sync source-backed, owner override; bounded alarm API doesn't enter suspend. | Retention across reboot/off, alarm interrupt and same-boot wake, bad-time recovery. |
| GPU / memory | Existing renderer/display/runtime PM and PSS/LOWMEM/HIGHMEM/cache/slab/growth observation. Reserved regions unchanged. | 480x360 smoothness, screen-off/wake power and repeated memory-growth tests. |
| OTA / recovery | Platform v1 root-only signed OTA/exact kernel; configuration contract v2 and battery/power schemas included. BOOTIMG remains preserving manual update. | Interrupted update/recovery/health/rollback later. This package includes only BOOTIMG/Y2ROOT and exact Hardware02 fallback, no data image. |

## Technical gaps, separate from qualification

Native S32/24-bit preservation and native 88.2/96 lack an exact MT6582 DL1 fetch,
interconnect and clock programming path in the inspected pinned source. The
conversion architecture is implemented; native modes are honestly false.
Runtime CPU power-down needs dormant context, CIRQ and GPT deadline ownership,
which cannot be substituted by suspend PCM. NCM is compiled/packaged with
activation integration pending. Deep suspend software exists but the captured
failure remains unresolved; same-boot restoration is not declared complete.
Actual net battery current/charge counter and pack temperature remain absent
when no hardware property exists. These are substantive limits, not merely
`qualified=false` labels for nonexistent implementations.

Astra's physical evidence and exact source/provenance index remain in
[the handoff](Y2-HARDWARE-FINAL-HANDOFF.md). Optional codec provenance/licensing
and native-audio register analysis remain in
[the source review](Y2-HARDWARE-FINAL-RADIO-AUDIO.md).
