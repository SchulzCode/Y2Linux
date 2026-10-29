#!/bin/sh
set -eu
cd /project
export Y2_ARTIFACT_TEST_ROOT=/build Y2_PRODUCTION_TEST=1 PYTHONPATH=/project/tests
# Current production profile and retained source-boundary regression modules.
# The test profile inventory and host prerequisites are documented in tests/README.md.
# The locked minimal host has libc headers but no distribution Linux UAPI.
# Export sanitised host UAPI from our already pinned kernel for native UI tests.
make -C /src O=/build/host-headers ARCH=x86 headers > /build/test-headers.log 2>&1
export C_INCLUDE_PATH=/build/host-headers/usr/include
python3 -m unittest tests.test_report_upload tests.test_report_fetch tests.test_first_owner tests.test_feature_qualification tests.test_release_composition tests.test_release_privacy tests.test_release_tool tests.test_system_update_fallback tests.test_radio_firmware -v
python3 -m unittest tests.test_hardware_ceiling -v
python3 -m unittest tests.test_bluetooth_aac_abr tests.test_bluetooth_sbc_abr tests.test_cpu_c3_uart tests.test_cpu_idle_harness tests.test_cpu_idle_completion tests.test_cpu_fix03 tests.test_cpu_fix02 tests.test_cpu_fix01 tests.test_cpu_final tests.test_cpu_dvfs tests.test_workload_qos tests.test_slow_idle tests.test_storage_recovery tests.test_hardware_final_contract -v
# The platform golden modules (tests.test_platform_differential, _argparse, _devices, _power_rpc, _observation,
# _suspend, _reports) run the Rust host build that Y2_PLATFORM_RUST names against tests/platform_goldens. This
# container has no cargo (and cannot run a glibc host build): they check the goldens and skip the runs, which
# tools/development/host-tests.sh makes on the host.
python3 -m unittest tests.test_integration_qualification tests.test_platform_v1_suspend tests.test_input_capture tests.test_usb_product tests.test_platform_measurement tests.test_platform_bluetooth tests.test_platform_transfer tests.test_platform_maintenance tests.test_platform_evidence tests.test_platform_qualification tests.test_platform_differential tests.test_platform_argparse tests.test_platform_devices tests.test_platform_power_rpc tests.test_platform_observation tests.test_platform_suspend tests.test_platform_reports -v
python3 -m unittest tests.test_storage_ceiling tests.test_sd_tuning tests.test_production_storage tests.test_filesystems tests.test_production_handover tests.test_readback_profiles tests.test_owner_data tests.test_reborn_receipts tests.test_mmc_requests tests.test_mmc_context tests.test_emmc_layout tests.test_mmc_transport tests.test_stock_msdos tests.test_usb_recover tests.test_usb_wake tests.test_usb_session tests.test_usb_state tests.test_usb_clock tests.test_usb_reconnect tests.test_usb_failure tests.test_audio tests.test_audio_precision_contract tests.test_audio_capture tests.test_highres_observations tests.test_dev_abi tests.test_dev_foundation tests.test_dev_i2c tests.test_baseline_pwrap tests.test_overlays tests.test_d08 tests.test_bootimg -v
# The owner's SFTP root (overlay sftp-server) on the compact Internal Storage record.
python3 -m unittest tests.test_sftp_root
# Power, radio and GPU subsystem/profile contracts remain separately identifiable.
PYTHONPATH=/project/tests python3 -m unittest tests.test_power tests.test_charger tests.test_poweroff_charger tests.test_watchdog_keepalive tests.test_charge_limits tests.test_charge_qualify tests.test_power_policy tests.test_offline_charging tests.test_power03 tests.test_connectivity tests.test_suspend_policy tests.test_suspend_audio tests.test_hp_level tests.test_codec_clock tests.test_gpu -v
# tests.test_reborn_splash additionally requires native libdrm and Pillow;
# it is a required packaging-host check, listed in tests/README.md.
python3 -m unittest tests.test_display_takeover tests.test_reborn_supervise -v
python3 -m unittest tests.test_audio_idle tests.test_pm_replay_trace tests.test_suspend_cpu_pm tests.test_dpm_stall_diagnostics tests.test_pmic_lowpower tests.test_haptics tests.test_speaker tests.test_sleep_relight tests.test_suspend_probe tests.test_usb_l1 tests.test_infra_pdn tests.test_dev_wheel tests.test_vproc_sleep tests.test_usb_savecurrent tests.test_golden_diagnostic -v
# POWER-FINAL-16: Defect B, DIS domain, stock DCM and golden sleep entry.
python3 -m unittest tests.test_usb_dma_unmask tests.test_display_domain tests.test_dcm tests.test_golden_apply tests.test_md1_gate tests.test_pmic_sleep tests.test_pwrap_gate tests.test_display_quiet_power tests.test_power_final_27 tests.test_power_final_33 tests.test_exfat_readahead tests.test_overlay_hashes -v
for script in /project/initramfs/production/init /project/initramfs/production/storage.sh /project/buildroot/board/y2/production-overlay/etc/init.d/* /project/buildroot/board/y2/production-overlay/usr/sbin/*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
for script in /project/buildroot/board/y2/production-overlay/usr/libexec/y2/*; do
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n "$script"
done
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n /project/tools/development/qualify-m4-power.sh
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/bin/busybox sh -n /project/buildroot/package/reborn/reborn-supervise
qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/y2-abi-check
# A BOOTIMG-only build stages rescue binaries; ALSA utilities remain in the
# unchanged verified ext4 image rather than this reduced staging directory.
if [ ! -f /build/userspace-source.json ]; then
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/aplay --version
    qemu-arm -cpu cortex-a7 -L /build/buildroot/target /build/buildroot/target/usr/bin/amixer --version
fi
