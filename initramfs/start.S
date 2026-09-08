/* SPDX-License-Identifier: GPL-2.0-only */
.syntax unified
.arm
.section .text.start,"ax",%progbits
.global _start
.type _start,%function
_start:
    mov r0, sp
    bl diag_start
    mov r0, #127
    mov r7, #1
    svc #0
1:  b 1b
.size _start, .-_start
.section .note.GNU-stack,"",%progbits
