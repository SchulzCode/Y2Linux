# Owner first storage flash failure — 2026-09-13

BOOTIMG transfer/checksum succeeded; ANDROID failed3154 after sparse detection;
USRDATA was not reached. This is not a physical readback or boot pass. Original
stock scatter was loaded with the three approved image overrides. PMT comparison
reports unchanged; that does not prove absence of implicit PMT writes or replace
protected before/after readback. Raw logs stay private (hardware identity fields).

Selected DA[5] stage2 is at bundled MTK_AllInOne_DA.bin offset0xa2210, length0x286bc,
load0x80000000. At0x80010284 it subtracts0xcac1 from chunk type; zero takes RAW,
result2 takes DONT_CARE, everything else logs unknown and returns0xc52=3154.
Thus FILL0xcac2 is rejected by this exact DA. Our first generated FILL follows
one1MiB RAW chunk, consistent with the observed early ANDROID failure. Stock
system image has1672RAW/13DONT_CARE chunks and no FILL. No DA patch or execution
was performed by the assistant. Public AOSP sparse definitions agree that FILL
is valid format but do not establish legacy DA support:
https://android.googlesource.com/platform/system/core/+/HEAD/libsparse/sparse_format.h

DA reports physical EMMC_USER0x1d2000000=7818182656 bytes=15269888sectors.
Historical Android exported0x1cff80000=7784103936 bytes=15203328sectors.
Difference0x2080000 remains outside layoutv1 allocation and is not reclaimed.
Native Linux capacity has not yet been measured. Correct the kernel card gate
for the physical count and root resolver for either explicitly observed disk
view; keep all write ranges and stock partition boundaries unchanged.
