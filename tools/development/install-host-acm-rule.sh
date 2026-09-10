#!/bin/sh
set -eu
[ "$(id -u)" = 0 ] || { echo 'Run through pkexec or sudo' >&2; exit 1; }
source_dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
install -m 0644 "$source_dir/70-y2linux-acm.rules" /etc/udev/rules.d/70-y2linux-acm.rules
udevadm control --reload-rules
udevadm trigger --action=change --subsystem-match=tty --sysname-match='ttyACM*'
udevadm settle
