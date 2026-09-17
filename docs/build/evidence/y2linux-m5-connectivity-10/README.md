# CONNECTIVITY-10 validation receipts

Final source `206f1da29d50a4a22cf01331a49b4519cd7a925f`, kernel
`6.18.0-y2linux-m5-connectivity-10`. BOOTIMG 6150144 bytes, SHA256
`d7fbb0808db953a7c8d26f3846815b7b5fbc67caa1b86248d7f992dba51f30a7`.
Retain CONNECTIVITY-07 root/data. The -09 fallback BOOTIMG is 6150144 bytes,
SHA256 `200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a`.

All **16 targeted Wi-Fi/connectivity tests** pass against the emitted -10
artifacts. The new regression executes the actual TX power producer, command
queue and completion callbacks. Replaying the unmodified -09 producer fails
the successful-TX completion assertion, as recorded separately.

A clean production build and targeted rebuild correcting the new diagnostic
message's integer format pass. The correction produces an identical BOOTIMG
to source `38e2473`; no second installation is required. Existing donor warnings
are retained; the corrected diagnostic rebuild emits no warning. ARM observer,
config/D08, emitted DT, memory bounds, sleep ABI, BOOTIMG, package validation
and all package SHA256 checks pass. The DT differs from -09 only in the one-byte
initramfs end adjustment; no memory/resource layout changes.

No root/data rebuild or new firmware input. Normal owner-installed hardware
now verifies [stable wlan0, two scans, runtime restart and BlueZ power-on with Wi-Fi](../../../hardware-evidence/2026-09-17-m5-connectivity10/README.md).
Connection, throughput/audio and full M5 qualification remain open.
