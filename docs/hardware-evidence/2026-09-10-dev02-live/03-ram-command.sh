#!/bin/sh
# Bounded allocator-only qualification; reserve at least 256 MiB of headroom.
set -eu
available=$(awk '/MemAvailable:/ {print $2}' /proc/meminfo)
[ "$available" -ge 524288 ] || { echo 'Need 512 MiB available'; exit 1; }
uptime
cat /proc/meminfo
dmesg
memtester 256M 1 &
test_pid=$!
(
    sleep 600
    if kill -0 "$test_pid" 2>/dev/null; then
        echo 'RAM TIME LIMIT: terminating memtester after 600 seconds'
        kill "$test_pid"
    fi
) &
watchdog=$!
(
    while kill -0 "$test_pid" 2>/dev/null; do
        sleep 20
        echo 'RAM MONITOR'
        uptime
        cat /proc/meminfo
        dmesg
    done
) &
monitor=$!
set +e
wait "$test_pid"
rc=$?
kill "$watchdog" "$monitor" 2>/dev/null
wait "$watchdog" "$monitor" 2>/dev/null
echo "MEMTESTER_EXIT=$rc"
uptime
cat /proc/meminfo
dmesg
exit "$rc"
