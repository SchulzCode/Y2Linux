# Storage02 DIAG01 offline evidence

Rescue build commit 8bf2d6404ee7f93c9244b124d7acea035e37e6c3; exact unchanged
Storage02 kernel/module from c2db4893b79eca25458b6adc318232c80dd99872.
Seven targeted tests plus ARM decoder success/rejection, real blkid probes,
shell syntax, unchanged hardware DT, newc, D08 memory, BOOTIMG format/bounds and
BOOTIMG-only scatter selection pass. Validation output's USB failure values are
synthetic fixture data, not newly measured hardware state.

Full package is ignored out/y2linux-storage-diag-01. SHA256SUMS describes that
package, not this small evidence directory. No new physical flash, readback or
kernel fix has occurred. Three on-screen pages are awaiting the owner's manual
BOOTIMG-only diagnostic test. Root/data images remain untouched by this package.
