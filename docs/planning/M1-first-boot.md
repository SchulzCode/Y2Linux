# M1 — Linux 6.18 First Boot

Canonical [milestone 2](https://github.com/SchulzCode/Y2Linux/milestone/2), created 2026-09-08. **Offline first wave complete; hardware first boot not performed and milestone remains open.** M0 recovery/launch gates remain open.

The owner explicitly changed execution for M1: the lead researches and implements directly, without delegation. Tiny sequential issues preserve reviewable checkpoints; none is state:luna-ready. All commits use the owner's configured identity and issues use the authenticated owner account. Platform work remains solely in Y2Linux.

| Checkpoint | Dependency | Completed change | Commit |
| --- | --- | --- | --- |
| [Y2B-201 / #8](https://github.com/SchulzCode/Y2Linux/issues/8) | Y2E-125/130 | Locked source/tool environment and ARMv7 smoke test | `fc53655` |
| [Y2B-205 / #9](https://github.com/SchulzCode/Y2Linux/issues/9) | #8 | D08 arithmetic validator and rejection cases | `fccf52d` |
| [Y2B-210 / #10](https://github.com/SchulzCode/Y2Linux/issues/10) | #9 | Minimal CPU0 config; D09 makes no-hyp selection enforceable | `9de3428` |
| [Y2B-215 / #11](https://github.com/SchulzCode/Y2Linux/issues/11) | #10 | D08 DT memory and researched CPU/GIC/GPT/UART foundation | `1e6a2b5` |
| [Y2B-220 / #12](https://github.com/SchulzCode/Y2Linux/issues/12) | #11 | Tiny deterministic diagnostic initramfs and QEMU user self-test | `927e457` |
| [Y2B-225 / #13](https://github.com/SchulzCode/Y2Linux/issues/13) | #12 | Actual ELF/DT/gzip layout checks, fixed physical base, appended payload | `950af2e` |
| [Y2B-230 / #14](https://github.com/SchulzCode/Y2Linux/issues/14) | #13 | Strict legacy MTK package and explicit LK read padding | `84d1bf2` |
| [Y2B-235 / #15](https://github.com/SchulzCode/Y2Linux/issues/15) | #14 | Clean reproducibility, actual corruption tests, retained result and launch-gate review | Final result commit containing this document |

[Result, exact versions/hashes/map](../build/first-boot-result.md) and [reproduction commands](../build/first-boot.md) are authoritative for this artifact. D08 limits are unchanged. Configuration resolution exposed a hidden upstream option; bounded research produced one Kconfig prompt-visibility patch (D09) rather than accepting a policy violation.

The [hardware launch-gate review](../knowledge/first-boot-launch-gates.md) identifies the next smallest missing proofs. No speculative peripheral/rootfs/SMP backlog or next implementation wave is created. The current output is an unsigned offline candidate. Closing the eight implementation checkpoints does not close M1, pass M0, authorize a DA, raw acquisition, flash, boot or restore, or prove the physical console. The next wave must start from launch-gate evidence and a concrete separately authorized experiment.
