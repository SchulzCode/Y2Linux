# DEV-02 physical live evidence, 2026-09-10

See [the qualification report](../../knowledge/y2linux-dev02-live-qualification.md).
Complete stdout/stderr is retained, including missing optional tools and failed attempts.
The physical source checkpoint is a34a360 plus pre-existing DEV-02 working changes.

- 01: original full baseline before live fixes; 02: initial optional timeout/evtest attempt (both absent); input-event*.bin/.txt: successful raw Linux evdev capture and decoding.
- 03: 256MiB exhaustive memtester, stopped at600s, exit143; 03-ram-command.sh is the exact command script. 11: completed256MiB mask0xff short pass, exit0. Periodic dmesg was retained during the first run; complete before/after logs during the second.
- 04–09: live DRM, SD, services, console, PMIC and host network diagnosis. Missing sysfs/tools are limitations, not invented hardware failures.
- 10/12: removable-root-guarded module/runtime fixes and /run chmod follow-up; original and final userspace manifests identify each revision.
- 13: successful scoped host ACM udev-rule installation (empty stdout, exit0 in tool transcript); the tool transcript and acm-before directory retain the initial EACCES attempt; acm-owner-reboot retains successful61463-byte LOG1 capture.
- reconnect-events.json and reconnect-*-{ssh,host}.txt: first unplug then owner restart, NOT clean reconnect. Uptime falls from1595.34 to33.32; original tmpfs token is gone. The old parser's token field contains the next uname line on this missing-token path; the raw SSH output establishes the failure and the tool is corrected afterward.
- 14–16: host disconnect/no-enumeration log and bounded SSH timeouts before owner restart.
- 17: complete baseline after the owner explicitly restarted; no assistant reboot occurred.
- 18/19: live cursor/checksum correction and five-second persistent SD recorder smoke test; no additional unplug.
- 20: final live service/hash/kernel check, while SSH remains connected.
- issues-before/after.json: requested evidence-based issue changes. #22–26 closed, #27/#28 remain open; M3/M4/M5 and GPU/final platform gates precede application work.
- validation*: targeted checks; the initial host root-selection test required the existing cached pyelftools path and then passed. Whole-tree diff whitespace warnings were pre-existing kernel patch context; scoped change check passes.

SHA256SUMS covers every retained evidence file except itself. Empty output files are intentional when commands succeeded silently. No private SSH keys, entropy seed contents, or internal eMMC data were copied into evidence.
