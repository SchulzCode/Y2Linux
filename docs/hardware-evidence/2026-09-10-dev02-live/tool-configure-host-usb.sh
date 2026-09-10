#!/bin/sh
# Persist the USB-only developer address for a verified, connected Y2 interface.
set -eu
iface=${1:?Usage: configure-host-usb.sh VERIFIED_Y2_INTERFACE}
properties=$(udevadm info --query=property --path="/sys/class/net/$iface")
printf '%s\n' "$properties" | grep -qx 'ID_USB_VENDOR_ID=0525'
printf '%s\n' "$properties" | grep -qx 'ID_USB_MODEL_ID=a4aa'
printf '%s\n' "$properties" | grep -qx 'ID_USB_SERIAL_SHORT=Y2LINUX-DEV-01'
[ "$(cat "/sys/class/net/$iface/address")" = 02:42:00:00:00:02 ]
connection=$(nmcli -g GENERAL.CONNECTION device show "$iface")
[ -n "$connection" ] && [ "$connection" != -- ]
nmcli -f connection.id,connection.interface-name,802-3-ethernet.mac-address,ipv4.method,ipv4.addresses,ipv4.never-default connection show "$connection"
nmcli connection modify "$connection" \
    802-3-ethernet.mac-address 02:42:00:00:00:02 \
    ipv4.method manual ipv4.addresses 10.42.0.2/24 ipv4.gateway '' \
    ipv4.never-default yes connection.autoconnect yes
nmcli -f connection.id,connection.interface-name,802-3-ethernet.mac-address,ipv4.method,ipv4.addresses,ipv4.never-default connection show "$connection"
