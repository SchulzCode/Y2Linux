# Current platform order — DEV-02 live checkpoint, 2026-09-10

[Physical qualification](../knowledge/y2linux-dev02-live-qualification.md):
core/Buildroot, expanded RAM, CPUs, input, display and SD are substantially
qualified. The first USB reconnect fails; resolve #27/#28 before closing M2.
The old five research slices #22–26 are satisfied within their narrow scope.

Next: M3 complete native audio → M4 full power → M5 connectivity → GPU/lima and
final reusable-platform qualification → Y2PlayerNative. Shared prerequisites
remain explicit; these are phase gates, not blanket implementation/flash authority.
No new BOOTIMG is prepared in the live pass. The former sequence below is history.

# Platform foundation work and execution order
**Current M2 workflow (2026-09-10):** one [M2-BASELINE-01](../build/m2-baseline-01-result.md)
integrates evidenced core controllers on Linux 6.18. Offline validated; stop for
one owner BOOTIMG flash, then inspect the complete USB log and fix independent
failures together. No separate INPUT-01/USBACM-04 prerequisite flash. M2 closure
still requires real hardware exit evidence; M3/M4/M5 stay separate.

The earlier sequences below are retained as historical checkpoints.


> **Additional evidence, 2026-09-10:** [research matrix](../knowledge/reverse-engineering-audit.md)
> turns SMP, wheel, power and display discovery into validation/adaptation work.
> Keep M2-INPUT-01 and USBACM-04 as pending hardware candidates; shared CCF,
> one PWRAP owner, EINT/I2C/DMA and RAM reconciliation remain the next providers.
> No memory expansion or later milestone is activated by external successes.

> **Donor adoption, 2026-09-09:** the owner's new implementation instruction
> supersedes the rigid order below. [Complete donor audit](../knowledge/donor-audit.md)
> selects GPIO navigation/evdev on D08 first while USB robustness remains open;
> corrected CCF/PWRAP/EINT/I2C and meaningful RAM reconciliation then unlock
> wheel/power, display and storage/rootfs by actual dependency. Reuse the existing
> #22–#32 owners. M1 complete; M2 active/exit blocked; M3/app deferred. Build
> reviewable Linux 6.18 candidates and stop for the owner's manual BOOTIMG flash.

> **2026-09-09 activation:** the owner authorized incremental M2 implementation
> with a stop before device changes and at each testable BOOTIMG. The
> [activation audit](roadmap-gap-audit.md#m2-activation-audit--2026-09-09) now
> prioritizes #27 plus the smallest VUSB/clock/PWRAP prerequisite from #23,
> then RAM adequacy/#22, input/#24, display/#25, SD/#26, rootfs and qualification.
> The original research scopes/order below remain historical context, not a
> prohibition on the newly authorized work. No hardware success is implied.

2026-09-08. [M1 core is achieved on the physical Y2](../knowledge/m1-runtime-hardware-result.md).
Exactly five issues are drafted below. None was executed today, and no M2
milestone, implementation or hardware work has begun. These are deliberately
bounded research tasks: present evidence does not yet justify guessing driver
contracts. A later instruction to work an ID means completing that issue's stated
research deliverable, not silently adding a driver or authorizing a flash.

| Order | Stable issue | One deliverable | Dependency interpretation |
| --- | --- | --- | --- |
| 1 | [Y2E-155 / #22 — Specify one safe static RAM expansion step](https://github.com/SchulzCode/Y2Linux/issues/22) | One exact additional-RAM policy or a precise NO-GO | Uses Y2E-130, Y2B-250 and current hardware evidence; D08 unchanged until a reviewed successor. |
| 2 | [Y2E-160 / #23 — Establish the MT6323 power and supply prerequisite map](https://github.com/SchulzCode/Y2Linux/issues/23) | Minimal PMIC transport/supply ownership contract | Follows the RAM decision; does not require actual expansion. |
| 3 | [Y2E-165 / #24 — Resolve the wheel and select-button event path](https://github.com/SchulzCode/Y2Linux/issues/24) | CW/CCW/select evdev implementation contract or bounded blocker | Uses supply/transport findings; full input and power drivers are not prerequisites for research. |
| 4 | [Y2E-170 / #25 — Specify a safe display handoff and backlight ownership boundary](https://github.com/SchulzCode/Y2Linux/issues/25) | One standard-interface takeover decision preserving observation | Uses power ownership and proven D13/D14; follows input research without requiring its driver. |
| 5 | [Y2E-175 / #26 — Specify a read-only removable-SD controller proof](https://github.com/SchulzCode/Y2Linux/issues/26) | One slot/controller read-only test contract or first missing prerequisite | Uses RAM, power and inherited display-DMA exclusions; keeps eMMC disabled. |

RAM headroom comes first, shared power dependencies next, then local controls
and display ownership, and finally removable storage once memory/DMA/supply
constraints are explicit. Removable SD is a smaller first storage surface than
internal eMMC. Proper panel/backlight support may need a later controller step;
working inherited pixels are not a driver contract. Power findings also feed
later audio without opening audio work now.

Each issue includes scope, out-of-scope, expected files/components, acceptance,
validation, stop conditions and unlocks. Stop at the specified evidence boundary;
record denied/missing evidence rather than forcing all-five implementation.
No follow-on issue is created automatically. Reprioritize only on returned facts.

For future localized code changes, use one clean build, directly affected tests,
D08/layout/BOOTIMG safety checks and final image size/hash. Material memory,
architecture or packaging changes and new hardware subsystems require the broader
relevant verification review. Do not routinely rehash all Linux sources or redo
ROM/recovery provenance, and do not repeat two builds after every small change.
Every future hardware trial still needs its own explicit authorization.

M1 core completion records the actual runtime result; it is not a release-quality
certification or retrospective full-suite execution. Physical UART research
Y2E-140 #16 stays open separately and does not erase achieved visual observation.

## Coverage extension, 2026-09-09

The [roadmap audit](roadmap-gap-audit.md) adds five deferred phase epics around this unchanged five-issue queue and the separately requested USB logging #27. No detailed tasks or new execution are added. Repeat the audit at each major milestone boundary before proceeding.
