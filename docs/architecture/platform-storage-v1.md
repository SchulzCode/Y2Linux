# Storage lifecycle, space and measurements

`/data` is persistent internal Y2DATA; `/media/sd` is optional removable media.
The root/data identity checks and partition geometry remain unchanged. All SD
mounts use the existing 11240000.mmc controller, one unique supported filesystem,
and `nosuid,nodev,noexec`. FAT/exFAT also use private file/directory masks.

`y2-media mount|unmount|status|check` is the lifecycle interface. Mount publishes
a boot-scoped mount ID, major/minor, UUID and CID claim. Status rejects missing
sources, changed UUIDs, missing claims and replacement mounts. Failed unmount
retains its claim and reports failure; there is no force/lazy unmount or new
mount stacked over the old one. Insertion callers must retry only after the old
mount has gone. `check` is an explicit, bounded, read-only check of unmounted
media. Repair and formatting are never automatic.

exFAT uses Linux 6.18's in-tree driver and Buildroot's pinned exfatprogs 1.2.5:
source SHA-256 `f27160dcc1ddd17c96cd41a6ceef7037adc2796ab5c5633d3d85cf532c3ee2f0`,
license hash `8177f97513213526df2cf6184d8ff986c675afb514d4e68a404010521b880643`.
No proprietary implementation is introduced. Upstream supplies the
[GPL tools](https://github.com/exfatprogs/exfatprogs/blob/master/README.md);
Microsoft documents [Linux exFAT integration](https://opensource.microsoft.com/blog/2019/08/28/exfat-linux-kernel).
ARM/image validation and actual cards remain separately gated. No bus mode,
clock, voltage or electrical assumption changes with this filesystem option.

`y2-platform space` publishes admission decisions in `/run/y2/storage.json`.
Cache, diagnostic and update allocations require Normal space and at least
96 MiB remaining; user-state/database checkpoints may use the reserve down to
32 MiB. Neither policy can make a failing filesystem writable. Applications
must retain write failures as failures and may not promise persistence based
only on this advisory snapshot. An allocation can race another writer.

`y2-platform space --cleanup` acts only on verified Y2DATA in LowSpace or
CriticalSpace. It removes at most 64 files / 8 MiB per invocation, only old
named Reborn artwork-cache files, old named diagnostic archives (retaining the
newest two), and named platform temporary cache files. All must be at least
one day old. Descriptor-relative opens refuse symlinks and subordinate mounts;
music, database, settings, bonds, keys and update transactions are excluded.
No image's nominal total flash size substitutes for Y2DATA's actual budget.

`y2-platform bench-storage --volume /data` or `--volume /media/sd` creates a
unique run directory under that volume's `.y2-bench`. Only newly created files
are accessed or removed. Directory descriptors pin operations to the original
filesystem; mount generation is rechecked between operations. Bounds are
1–128 MiB, 1–1024 operations and a 1–1800 second deadline (defaults 16 MiB,
64 operations, 300 seconds). At least max(96 MiB, 10%) remains reserved before
starting. No raw block test, global cache drop, format or real-library write.

Sequential and random read/write use 4/16/64 KiB and 1 MiB transfers with nonzero
readback verification. Results distinguish buffered operations and file-scoped
advisory cache eviction. Write throughput includes final fdatasync; operation
latency distributions describe individual calls. Separate file fsync/fdatasync,
create/stat/rename/delete and file-fsync/rename/directory-fsync transactions have
p50/p95/p99/max, operations/sec and decimal MB/sec. Unsupported directory fsync
is Unavailable, never a durability pass. Collection/identity-check overhead is
included in throughput. An I/O error or deadline retains completed results and
marks the run FAILED. Kernel uninterruptible I/O remains outside userspace
deadline guarantees. Scratch remnants from killed runs are not auto-deleted.

These are measurement tools, not target measurements. Physical SD removal,
full media, filesystem errors, card compatibility, long I/O tails and flush /
electrical power-loss durability remain PHYSICAL_GATE. Host process-crash
recovery must never be presented as electrical durability.
