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

# F23: the normal restart stops Reborn and turns the light off before
# reboot(2); recovery restarted with its UI still flipping frames (≈9/s) on a
# lit panel, and on GC9503V players the new system's first boots died before
# their logger. Hand the bootloader a still, dark display either way.
recovery_ui=
recovery_display_off() {
    for bl in /sys/class/backlight/*/bl_power; do [ ! -w "$bl" ] || echo 4 > "$bl"; done
    [ -n "$recovery_ui" ] || return 0
    kill "$recovery_ui" 2>/dev/null
    local waited=0
    while [ "$waited" -lt 20 ] && kill -0 "$recovery_ui" 2>/dev/null; do
        sleep 0.05; waited=$((waited+1))
    done
    kill -9 "$recovery_ui" 2>/dev/null
    wait "$recovery_ui" 2>/dev/null
    recovery_ui=
    # The UI's DRM last close restores the console buffer: let that land dark.
    for bl in /sys/class/backlight/*/bl_power; do [ ! -w "$bl" ] || echo 4 > "$bl"; done
    sleep 0.2
}

# F34: players froze during an install and showed "Step 1 of 4" for hours
# until someone held Power (a restart then installed). The SoC watchdog (31 s)
# now does that restart: it is fed while the installer reports progress and
# starves when the kernel freezes or the installer shows nothing new for two
# minutes. The request stays set, so the next start resumes (or restores the
# old system), as after Power. Only while the installer runs.
recovery_watchdog=
recovery_watchdog_start() {
    [ -c /dev/watchdog ] || return 0
    rm -f /run/y2/recovery-watchdog-stop
    (
        exec 3>/dev/watchdog || exit 0
        last= still=0
        while [ ! -e /run/y2/recovery-watchdog-stop ]; do
            now=$(cat /run/y2/recovery-progress 2>/dev/null)
            if [ "$now" = "$last" ]; then still=$((still+5)); else still=0; fi
            last=$now
            if [ "$still" -ge 120 ]; then
                echo "<3>Y2RECOVERY: no progress for 120 s (${now:-none}); watchdog restart" > /dev/kmsg
                [ ! -c /dev/pmsg0 ] || echo "watchdog no progress ${now:-none}" > /dev/pmsg0
                sync
                while :; do sleep 60; done
            fi
            printf . >&3
            sleep 5
        done
        printf V >&3
    ) &
    recovery_watchdog=$!
}
recovery_watchdog_stop() {
    [ -n "$recovery_watchdog" ] || return 0
    touch /run/y2/recovery-watchdog-stop
    wait "$recovery_watchdog" 2>/dev/null
    recovery_watchdog=
}

# The previous start's trail and console from pstore (a run that froze, then
# the watchdog or Power restarted), kept next to the installer log (F34).
recovery_keep_previous_boot() {
    mount -t pstore pstore /sys/fs/pstore 2>/dev/null || return 0
    {
        echo "== this start $(cat /proc/sys/kernel/random/boot_id) $(cat /sys/firmware/y2_boot/metadata 2>/dev/null)"
        for part in pmsg-ramoops-0 dmesg-ramoops-0 console-ramoops-0; do
            [ ! -s "/sys/fs/pstore/$part" ] || { echo "== $part"; /bin/busybox head -c 262144 "/sys/fs/pstore/$part"; }
        done
    } > /newdata/logs/recovery-previous-boot.txt 2>/dev/null
    umount /sys/fs/pstore
}

# Keep the evidence, then turn off or, with the owner's switch, restart.
recovery_off() {
    if [ -n "$recovery_data_mounted" ]; then
        recovery_evidence
        umount /newdata 2>/dev/null
    fi
    sync
    sleep "${1:-12}"
    recovery_display_off
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
recovery_screen 'Installing Update' "Don't turn off the player." 'This takes less than a minute.'
if [ -x /sbin/y2-recovery-ui ]; then /sbin/y2-recovery-ui & recovery_ui=$!; fi
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
mkdir -p /newdata/logs
recovery_keep_previous_boot
/bin/busybox timeout 10 /bin/busybox seedrng --seed-dir=/newdata/system/entropy > /dev/kmsg 2>&1 || :
recovery_keep_radio_firmware
y2_stage recovery_install

attempt=0
# F34 (8.23 test build): the installer wrote "installed" (journal saved, the
# recovery request cleared) and then never exited; recovery waited for it.
# A finished install or restore is not waited for longer than 30 s.
recovery_core() {
    /sbin/y2-update-core recovery > /run/y2/recovery-result.json &
    core=$!
    finished=0
    while kill -0 "$core" 2>/dev/null; do
        case "$(cat /run/y2/recovery-progress 2>/dev/null)" in
            '100 installed'*|'100 restored'*) finished=$((finished+1)) ;;
        esac
        if [ "$finished" -ge 30 ]; then
            recovery_log "installer finished but did not exit; continuing"
            [ ! -c /dev/pmsg0 ] || echo "core finished, no exit" > /dev/pmsg0
            case "$(cat /run/y2/recovery-progress)" in
                '100 installed'*) echo '{"outcome":"installed","exit":"hung"}' ;;
                *) echo '{"outcome":"rolled_back","exit":"hung"}' ;;
            esac > /run/y2/recovery-result.json
            return
        fi
        sleep 1
    done
    wait "$core"
}

while :; do
    recovery_watchdog_start
    recovery_core
    recovery_watchdog_stop
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
    recovery_screen 'Installing Update' "Don't turn off the player." 'This takes less than a minute.'
done

case "$outcome" in
    installed)
        if [ -n "$recovery_restart" ]; then
            recovery_screen 'Update installed' '' 'Starting the new version.'
            recovery_off 1
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
