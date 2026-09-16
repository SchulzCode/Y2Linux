# CONNECTIVITY-07: identify factory storage through mounted root/data

CONNECTIVITY-06 is running, but this boot assigns internal eMMC `mmcblk1`.
Mounted Y2ROOT/Y2DATA and logical capacity are correct; the factory provider
assumes `mmcblk0` and exits before radio activation. This is a device-enumeration
bug, not missing calibration or a reason to rewrite factory partitions.

The provider now resolves the block parents of the mounted `/` and `/data`
through `/sys/dev/block/<major>:<minor>`. It requires partitions 5 and 7 on the
same `mmcblkN` parent, type `MMC`, an actual matching block-node device number,
and the unchanged 7784103936-byte logical geometry. It opens that parent with
O_RDONLY/O_NOFOLLOW and only then performs the existing bounded protected reads.
Wrong partition, another card, SD, absent node, regular file, mismatched device
number or geometry fails closed. No `/dev/mmcblk0` alias or guessed fallback exists.

The regression exercises the production resolver for mmcblk0 through mmcblk7
and twelve failure conditions. Factory decoding, private outputs, identities,
firmware and calibration contents are unchanged. The CONNECTIVITY-06 TX fix and
all M4 hardware ownership are retained; its physical transport result is pending.

Rebuild the existing production Y2ROOT helper and package BOOTIMG + Y2ROOT with
the preserving scatter. Keep Y2DATA and protected partitions untouched. Verify
the new helper actually resides in the ext4 payload. After manual installation,
inspect provider/MD/BTIF/WMT startup over SSH, then proceed with the original M5
qualification as interfaces become usable. M5 remains open; no new audit/inventory.
