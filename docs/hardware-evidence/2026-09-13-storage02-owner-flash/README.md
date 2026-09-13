# Storage02 owner flash and power-on report — 2026-09-13

The owner manually flashed the Storage02 package. SPFT records Download Only,
S_DONE(0), 96 seconds, and 1.28 GB total. The raw-image transport passed the
previous sparse-parser failure. Full private tool logs are retained separately;
this is transfer evidence, not physical readback/hash or boot qualification.

The owner initially reported a black, unresponsive device, then reported that
holding Power + Volume Up made it work. Record this as an owner-observed recovery
from that state, not a characterized reset mechanism or general button recipe.
The owner confirms USB is connected and no SD is inserted. The resulting
screen/boot stage and normal-system state have not yet been specified. Do not infer a kernel fix or failed production boot cause.

Host inspection after the report found no Y2 USB device, ACM node or USB Ethernet
interface. Therefore SSH verification was unavailable. Earlier host USB events
show the MediaTek preloader during the flash at 14:47:06 and disconnect at
14:48:44. Subsequent preloader appearances at 14:50:58 and 14:53:10 each
disconnect after three seconds. These prove preloader USB response, not Linux
enumeration. No Linux USB gadget appearance was observed.

Pending: exact current screen and boot stage, Linux/kernel identity, internal
Y2ROOT/Y2DATA mount evidence, no-SD boot, retained display/input/ALSA/USB/SSH,
writability/persistence, protected readbacks and rescue-negative qualification.
Production Storage v1 remains active. No assistant eMMC write or SPFT execution.
The original AUDIO-02 BOOTIMG hash was rechecked and still matches its documented
BOOTIMG-only SD fallback. No restoration was performed.
