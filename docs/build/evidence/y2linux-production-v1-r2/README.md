# Production Storage v1 Storage02 retry evidence

Built from c2db4893b79eca25458b6adc318232c80dd99872; release 0.1.0-storage.2. Replaces the failed Storage01
transport with raw ext4 images and corrects the physical eMMC capacity guard.
All stock boundaries and protected write exclusions remain unchanged.

26 regression tests, ARM ABI/ALSA/shell checks, artifact validation, ext4 integrity,
geometry/identity/manifest/raw transport checks and synthetic readback rejection
pass. No physical retry, no-SD boot or protected-byte comparison is claimed.
SHA256SUMS describes the ignored full out/y2linux-production-v1-r2 package,
not this small retained evidence directory. No private key or raw device log is
included. The owner performs all SPFT/readback operations.
