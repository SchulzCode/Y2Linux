# Storage06 owner boot result

The owner flashed the single corrected production BOOTIMG and reported that it
works. USB identity reports Linux6.18.0-y2linux-storage06. The expected local
BOOTIMG hash is `3ba809e44bd9c945c994209d1d93f41fe0da9259a303487dd6ecf49724bf49a8`;
no full on-device BOOTIMG hash was read back.

The20-second read-only LOG1 capture contains364032 bytes, valid protocol header
and contiguous kernel sequences0–2094. Raw bytes stay private; [capture metadata](capture.json),
[selected lines](acm-excerpt.txt) and [machine-readable result](result.json) are retained.

* Raw SEC_COUNT15269888, stock offset23552, visible disk15203328 sectors.
* Sector0, EBR1 and EBR2 read55aa. Upstream parser exposes p5/p7 at expected sizes.
* Both ext4 identities match; preflight/schema/ABI/e2fsck pass. Root/data mount rw
  and switch to Buildroot at7.878653s. Snapshot shows /dev/mmcblk0p5 on / and
  /dev/mmcblk0p7 on /data; no removable block device is present.
* Four CPUs and954376KiB RAM; no eMMC guard rejection, I/O error or rescue entry
  in this capture. The expected legacy oversized p8 is clipped at disk end.
* USB ECM announces10.42.0.1/24. The host initially selected DHCP; its active
  USB profile was corrected to10.42.0.2/24, never-default, without reconnecting.
  Dropbear accepts the TCP connection but rejects the existing owner key.

Two preflight-unmount warnings name inode1 through ext4_mb_release. Source
mballoc.c explicitly creates its synthetic buddy-cache inode as EXT4_BAD_INO=1;
ext4_destroy_inode tests its orphan state. The32-bit inode state is separate
from i_flags, which allocation clears. This points to synthetic-inode state
initialization, not a readback of corrupt on-disk inode1. Both subsequent
filesystem checks report clean. Retain this for later filesystem qualification;
no additional kernel variant or table repair is justified by this warning.

The owner requests ~/.ssh/y2linux_ed25519.pub, public SHA256
`bf9cc937fb5dcee8569514b674fb9b3d5ba25396162efac7e8854f88bfe98c25`.
The existing template instead has public SHA256
`46d122f86bdecb4daf3e435ec6098b5b041575f0869a8e87bb4851ff3a2a4fa8`.
The actual SSH attempt used only the explicitly selected existing identity and
was rejected. No private-key contents were inspected/copied/packaged/committed.
The owner confirms Y2DATA contains only initial system state, permitting a
manual data-initialization correction while BOOTIMG/Y2ROOT stay in place.

This confirms the internal boot correction, not authenticated scratch writes,
repeated boots, power-fail behavior or complete platform release qualification.
#33 stays ACTIVE; M4/radio/GPU/reconnect/application scope is unchanged.
