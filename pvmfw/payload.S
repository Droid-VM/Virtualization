/*
 * Copyright 2022 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

.macro mov_i, reg:req, imm:req
	movz \reg, :abs_g3:\imm
	movk \reg, :abs_g2_nc:\imm
	movk \reg, :abs_g1_nc:\imm
	movk \reg, :abs_g0_nc:\imm
.endm

.set .L_SCTLR_EL1_RES1, (0x1 << 11) | (0x1 << 20) | (0x1 << 22) | (0x1 << 28) | (0x1 << 29)

/**
 * Disable the exception vector, caches and page talbe and then jump to the payload at the given
 * address, passing it the given FDT pointer.
 *
 * x0: FDT address to pass to payload
 * x1: Payload address
 */
.global start_payload
start_payload:
        /* Move payload address to a higher register and zero out parameters other than x0. */
        mov x30, x1
        mov x1, #0
        mov x2, #0
        mov x3, #0

        /* Zero out remaining registers to avoid leaking data. */
        mov x4, #0
        mov x5, #0
        mov x6, #0
        mov x7, #0
        mov x8, #0
        mov x9, #0
        mov x10, #0
        mov x11, #0
        mov x12, #0
        mov x13, #0
        mov x14, #0
        mov x15, #0
        mov x16, #0
        mov x17, #0
        mov x18, #0
        mov x19, #0
        mov x20, #0
        mov x21, #0
        mov x22, #0
        mov x23, #0
        mov x24, #0
        mov x25, #0
        mov x26, #0
        mov x27, #0
        mov x28, #0

        /* Ensure that all memory access has completed before we disable the MMU. */
        dsb nsh

        /* Disable the MMU and cache, and set other settings to valid warm reset values. */
        mov_i x29, .L_SCTLR_EL1_RES1
        msr sctlr_el1, x29
        isb
        msr ttbr0_el1, xzr

        isb
        dsb nsh

        /* Jump into the payload. */
        br x30
