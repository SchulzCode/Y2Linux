/* SPDX-License-Identifier: GPL-2.0-only */
#include <linux/linkage.h>
 .syntax unified
 .arm
 .text
 .align 5
/* Boot ROM enters in ARM with MMU off. No stack, DRAM data, cache, GIC or timer
 * dependency. Record reset dispatch before Linux restores its own context. */
ENTRY(y2_cpu_resume)
 movw r0, #0xdd00
 movt r0, #0x0010
 movw r1, #0x5253
 movt r1, #0x5932
 str r1, [r0]
 dsb sy
 b cpu_resume_arm
ENDPROC(y2_cpu_resume)
