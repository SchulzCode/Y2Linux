#!/bin/sh
# Functional checks of a platform test build on the player, after its OTA install (run there: ssh … 'sh -s' < this file).
# SPDX-License-Identifier: GPL-2.0-only
echo "== update"; y2-platform update status | tr ',' '\n' | grep -E '"state"|"sequence"' | head -3
echo "== daemons"; ps -o pid,rss,args | grep -E "[y]2-platform .*-daemon|[/]usr/bin/reborn$"
echo "== wifi power"; cat /run/y2/wifi-power.json; echo; iw dev wlan0 get power_save
echo "== power.json rewrites in 20 s"; i=0; last=""; n=0; while [ $i -lt 40 ]; do m=$(ls -i /run/y2/power.json | awk '{print $1}'); [ "$m" != "$last" ] && n=$((n+1)); last=$m; usleep 500000; i=$((i+1)); done; echo "$n distinct"
echo "== usb0 address"; y2-platform status system | tr ',' '\n' | grep '"ipv4"'; ip -4 -o addr show dev usb0 | awk '{print $4}'
echo "== wifi"; y2-platform status wifi | tr ',' '\n' | grep -E '"association"|"rssi_dbm"|"state": "Online"' | head -4
echo "== storage.json (written by the service daemon)"; ls -le /run/y2/storage.json 2>/dev/null || ls -l /run/y2/storage.json; tr ',' '\n' < /run/y2/storage.json | grep -E '"state"|space_state' | head -3
echo "== ssh"; tr ',' '\n' < /run/y2/ssh.json | grep '"state"'
echo "== health"; y2-platform health | tr ',' '\n' | grep '"state": "[A-Z]' | sort | uniq -c
