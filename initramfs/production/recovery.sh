# SPDX-License-Identifier: GPL-2.0-only
# Y2 recovery image (OTA-35). init sources this when the kernel was loaded from
# RECOVERY (y2.recovery=1 in that image's own DTB). It installs or restores
# BOOTIMG + Y2ROOT through y2-update-core, records what happened on Y2DATA and
# powers off, or restarts into the result when the owner's restart switch is
# set. It never starts the normal system itself, formats storage or reboots in
# a loop. Uses init's y2_stage, start_observer and y2_find_partition.

recovery_screen() {
    printf '%s\n' "$@" > /run/y2/recovery-screen.tmp &&
        mv -f /run/y2/recovery-screen.tmp /run/y2/recovery-screen
}

recovery_log() {
    echo "<6>Y2RECOVERY: $*" > /dev/kmsg
}

# Restart into the result (OTA-46 default). The owner's override
# /data/updates/restart.json {"mode": "poweroff"}, read while Y2DATA is
# mounted, turns off instead. Before Y2DATA is mounted nothing is restarted.
recovery_restart=
recovery_restart_mode() {
    recovery_restart=1
    grep -q '"mode": *"poweroff"' /newdata/updates/restart.json 2>/dev/null && recovery_restart=
}

# The recovery's evidence on Y2DATA (the normal system's diagnostics read it):
# result, kernel log and the display path's register snapshot.
recovery_evidence() {
    mkdir -p /newdata/logs
    cp /run/y2/recovery-result.json /newdata/updates/recovery-last.json 2>/dev/null
    dmesg > /newdata/logs/recovery-last.log 2>/dev/null
    cat /sys/kernel/debug/y2_display_snapshot > /newdata/logs/recovery-display.txt 2>/dev/null
    sync
}

# Keep the evidence, then turn off or, with the owner's switch, restart.
recovery_off() {
    if [ -n "$recovery_data_mounted" ]; then
        recovery_evidence
        umount /newdata 2>/dev/null
    fi
    sync
    sleep "${1:-12}"
    if [ -n "$recovery_restart" ]; then
        recovery_log 'restart'
        reboot -f
    fi
    recovery_log 'power off'
    poweroff -f
    # poweroff never returns on this hardware; keep PID1 alive if it does.
    while :; do sleep 60; done
}

# Unknown or manual-repair state: keep the request (LK returns here) and keep
# PID1 with a console shell, exactly like the BOOTIMG rescue. On battery the
# screen would stay on until the cell is empty (V1 code review B8): after ten
# minutes without a charger the player turns off; Power brings this back.
recovery_stuck() {
    recovery_log "$*; staying in recovery"
    recovery_screen 'Update needs attention' "$1" '' 'Connect the player' 'to a computer.'
    [ -z "$recovery_data_mounted" ] || recovery_evidence
    (
        idle=0
        while [ "$idle" -lt 300 ]; do
            if charger_online; then idle=0; else idle=$((idle+1)); fi
            sleep 2
        done
        recovery_log 'stuck on battery for ten minutes; power off'
        recovery_screen 'Update needs attention' "$1" '' 'Turning off to save the battery.' 'Press Power to see this again.'
        recovery_restart=
        recovery_off 8
    ) &
    while :; do
        /bin/busybox sh </dev/console >/dev/console 2>&1
        sleep 1
    done
}

# RELEASE-49: images carry no MediaTek radio firmware. Before a new root is
# written, keep the reviewed files of the old root (images before 49 carried
# them) on Y2DATA, where /lib/firmware/mediatek/mt6582 points. Hash-checked;
# never fatal: without them the owner copies system.img into "Firmware".
RADIO_FIRMWARE='4def37522e2bfd2d4b78e01bf9f567a37e3f67ab3d7b6646ffb317f5b2b49b82  WIFI_RAM_CODE_MT6582
cea0a94b8a88e9c24e6835446ebc3106d7b100c3709ea98e8bb7368689c8a2b5  WMT_SOC.cfg
fa3be4a92540296de512d64f56272746be10bdfa2db571c8ceada2d40cd616f0  mt6572_82_patch_e1_0_hdr.bin
96eb25489305bd472805db1da33bcc0acd37b5bb21cceda98d7e88ba1179ca37  mt6572_82_patch_e1_1_hdr.bin
5059775975cbf6ab74c43978ca8f65d9a274b83585f456134e39b09f6dc7a4f1  modem_1_2g_n.img
eb5e87224e3a493e825179f547fc13924f564a42730ea821b4359b5b36f4defe  WIFI.defaults
f224ff4abfb45a612ef03249e87e4ecfa44b7c1f9f19ee3cc78ae354764d9472  BT.defaults'
radio_firmware_ok() {
    [ -d "$1" ] && [ ! -L "$1" ] &&
        (cd "$1" && printf '%s\n' "$RADIO_FIRMWARE" | /bin/busybox sha256sum -c -s -)
}
recovery_keep_radio_firmware() {
    target=/newdata/firmware/mt6582
    radio_firmware_ok "$target" && return 0
    rootdev=$(y2_find_partition Y2ROOT 79324c69-6e75-4801-8000-000000000101 166912 1679360)
    [ -n "$rootdev" ] || return 0
    mkdir -p /oldroot
    mount -t ext4 -o ro,noload "$rootdev" /oldroot 2>/dev/null || return 0
    source=/oldroot/lib/firmware/mediatek/mt6582
    if radio_firmware_ok "$source"; then
        mkdir -p /newdata/firmware
        /bin/busybox rm -rf /newdata/firmware/.mt6582.keep
        if mkdir -m 700 /newdata/firmware/.mt6582.keep &&
            cp "$source"/* /newdata/firmware/.mt6582.keep/ &&
            radio_firmware_ok /newdata/firmware/.mt6582.keep; then
            /bin/busybox rm -rf "$target"
            mv /newdata/firmware/.mt6582.keep "$target" && sync
            recovery_log 'radio firmware kept on Y2DATA'
        else
            /bin/busybox rm -rf /newdata/firmware/.mt6582.keep
            recovery_log 'radio firmware copy failed'
        fi
    fi
    umount /oldroot 2>/dev/null
}

charger_online() {
    for supply in /sys/class/power_supply/*; do
        case "$(cat "$supply/type" 2>/dev/null)" in USB|Mains) ;; *) continue ;; esac
        [ "$(cat "$supply/online" 2>/dev/null)" = 1 ] && return 0
    done
    return 1
}

y2_stage recovery
recovery_data_mounted=
recovery_screen 'Installing Update' "Don't turn off the player." 'This takes 1-2 minutes.'
[ -x /sbin/y2-recovery-ui ] && /sbin/y2-recovery-ui &
recovery_log "$(cat /sys/firmware/y2_boot/metadata 2>/dev/null)"
echo 1 > /sys/firmware/y2_boot/normal_boot || recovery_stuck 'Storage did not start'
if [ -w /sys/devices/system/cpu/cpufreq/policy0/scaling_governor ]; then
    echo schedutil > /sys/devices/system/cpu/cpufreq/policy0/scaling_governor
fi
start_observer
n=0
while [ "$n" -lt 800 ]; do
    datadev=$(y2_find_partition Y2DATA 79324c69-6e75-4801-8000-000000000102 2104320 1638400)
    [ -n "$datadev" ] && break
    n=$((n+1))
    sleep 0.05
done
[ -n "$datadev" ] || recovery_stuck 'Storage not found'
e2fsck -p "$datadev" > /dev/kmsg 2>&1
[ "$?" -le 1 ] || recovery_stuck 'Storage check failed'
mount -t ext4 -o rw,nosuid,nodev "$datadev" /newdata || recovery_stuck 'Storage did not mount'
recovery_data_mounted=1
[ "$(cat /newdata/.y2data-schema 2>/dev/null)" = 1 ] || recovery_stuck 'Unknown storage format'
recovery_restart_mode
/bin/busybox timeout 10 /bin/busybox seedrng --seed-dir=/newdata/system/entropy > /dev/kmsg 2>&1 || :
recovery_keep_radio_firmware
y2_stage recovery_install

attempt=0
while :; do
    /sbin/y2-update-core recovery > /run/y2/recovery-result.json
    outcome=$(sed -n 's/.*"outcome":"\([a-z_]*\)".*/\1/p' /run/y2/recovery-result.json)
    recovery_log "outcome=${outcome:-none} $(cat /run/y2/recovery-result.json)"
    [ "$outcome" = needs_power ] || break
    # A low battery and no charger: wait for one (ten minutes), else turn off.
    # The request stays set, so the next power-on returns here.
    recovery_screen 'Connect the charger' 'The battery is too low' 'to install the update.'
    attempt=0
    until charger_online || [ "$attempt" -ge 300 ]; do
        sleep 2
        attempt=$((attempt+1))
    done
    # Always off here: a restart would come straight back and drain the battery.
    charger_online || { recovery_screen 'Battery too low' 'Charge the player, then' 'press Power to continue.'; recovery_restart=; recovery_off 10; }
    recovery_screen 'Installing Update' "Don't turn off the player." 'This takes 1-2 minutes.'
done

case "$outcome" in
    installed)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Update installed' '' 'Starting the new version.'
            recovery_off 4
        fi
        recovery_screen 'Update installed' '' 'The player turns off now.' 'Press Power to start.'
        recovery_off ;;
    rolled_back)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Update not installed' 'The previous version' 'was restored.' '' 'Restarting.'
            recovery_off 8
        fi
        recovery_screen 'Update not installed' 'The previous version' 'was restored.' '' 'Press Power to start.'
        recovery_off ;;
    failed_unchanged)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Update not installed' 'Nothing was changed.' '' 'Restarting.'
            recovery_off 8
        fi
        recovery_screen 'Update not installed' 'Nothing was changed.' '' 'The player turns off now.' 'Press Power to start.'
        recovery_off ;;
    probe)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Recovery works' '' 'Restarting.'
            recovery_off 4
        fi
        recovery_screen 'Recovery works' '' 'The player turns off now.' 'Press Power to start.'
        recovery_off ;;
    nothing)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Nothing to install' '' 'Restarting.'
            recovery_off 4
        fi
        recovery_screen 'Nothing to install' '' 'The player turns off now.' 'Press Power to start.'
        recovery_off 6 ;;
    *)
        recovery_stuck 'The update stopped' ;;
esac
