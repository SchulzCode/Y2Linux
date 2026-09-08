# Reproduce the offline first-boot artifact

Host requirements and locked package/source provenance are in [environment.md](environment.md). The source tree is upstream v6.18 plus the single [D09 Kconfig visibility overlay](kernel-policy.md), with no C/assembly kernel patch. This command sequence never runs ADB, a flasher or an ARM system emulator.

```sh
python3 tools/build/prepare.py --offline
python3 tools/build/full_source_check.py
python3 tools/build/run.py --output out/my-clean-build -- sh /project/tools/build/build.sh
```

Use preparation without `--offline` only to fetch missing checksum-locked inputs. The actual build runs in a private networkless/device-isolated userspace. Choose a fresh output directory; existing vmlinux or BOOTIMG makes the end-to-end command fail rather than reuse a stale product. No RAM limits or config switches are automatically relaxed on failure.

The pipeline resolves/checks config, compiles/tests the tiny ARM PID1, creates deterministic newc/gzip, builds Image/zImage, compiles DTS with actual gzip length, validates real ELF/DT/compression/layout evidence, appends DTB, packages/independently checks BOOTIMG, and runs all nine tests including actual-artifact mutations. The only execution of ARM code is QEMU **user-mode** `--selftest`, which exits before privileged operations. It is not a Linux/Y2 boot test.

Output includes `BOOTIMG.img`, `Image`, `zImage`, `zImage-dtb`, `y2.dtb`, `init`, `initramfs.cpio[.gz]`, `kernel.config`, `layout.json`, kernel/compressed ELFs below `kernel/`, and build/validation/test logs. The manifest contains decimal integer physical addresses; the [recorded result](first-boot-result.md) renders exact half-open intervals in hex. `tools.validation.artifacts` verifies the actual kernel/compressed ELF symbols and their corresponding byte images, the embedded gzip/size/BSS table, LC1 stack, every approved DT node/property and all D08 arithmetic. `tools.validation.bootimg` independently decodes the [legacy format](bootimg-format.md).

`full_source_check.py` compares all 91,166 upstream files/links against the locked archive; safe tar extraction normalizes redundant `./` in relative link targets, which the verifier accounts for explicitly. File contents must be byte-identical, and extra/missing paths fail. The Kconfig overlay is separately hash/diff checked by the runner and mounted read-only from the cache, outside writable build output. Never call a cached source tree pristine solely because 31 earlier audit files match.

The current UART0 correction and historical UART3 candidate each had two clean builds compared byte-for-byte across 11 outputs, including both ELF symbol containers and resolved config. A focused test-fixture correction handled options omitted entirely by Kconfig when disabled; no product bytes changed. Nine tests pass with no skips in complete builds. DT compilation and kernel logs contain no warning/error diagnostics. This is dtc syntax plus strict independent board-policy validation; a full dt-schema conformance run is not claimed. The new board-compatible string is local and not an upstream-accepted binding submission.

Stop on any failed checksum, config, architecture, binary, layout, test or reproducibility check. Return to the relevant research/architecture boundary; do not enlarge RAM, import LK ATAGs or enable peripherals to obtain a passing image. Hardware launch separately requires [all recorded gates](../knowledge/first-boot-launch-gates.md).
