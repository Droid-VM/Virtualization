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

/**
 * This is a generic entry point for an image. It carries out the operations
 * required to prepare the loaded image to be run. Specifically, it zeroes the
 * bss section using registers x25 and above, prepares the stack, enables
 * floating point, and sets up the exception vector.
 */
.section .init.entry, "ax"
.global entry
entry:
	/* Zero out the bss section. */
	adrp x29, bss_begin
	add x29, x29, :lo12:bss_begin

	adrp x30, bss_end
	add x30, x30, :lo12:bss_end

0:	cmp x29, x30
	b.hs 1f

	stp xzr, xzr, [x29], #16
	b 0b

1:	/* Prepare the stack. */
	adr x30, boot_stack_end
	mov sp, x30

	/* Disable trapping floating point access in EL1. */
	mov x30, #(0x3 << 20)
	msr cpacr_el1, x30
	isb

	/* Set up exception vector. */
	adr x30, vector_table_el1
	msr vbar_el1, x30

	/* Set up the page table. */
	bl init_mm

	/* Enable MMU and caches. */
	bl mm_enable

	/* Call into Rust code. */
	bl main

	/* Loop forever waiting for interrupts. */
2:	wfi
	b 2b

/**
 * Applies the memory management configuration to the CPU, preserving x0 along
 * the way.
 */
mm_enable:
	/*
	 * Invalidate any potentially stale local TLB entries before they start
	 * being used.
	 */
	tlbi alle2
	tlbi vmalls12e1

	/*
	 * Load and apply the memory management configuration.
	 */
	adrp x6, mm_config
	add x6, x6, :lo12:mm_config

	ldp x1, x2, [x6]
	ldp x3, x4, [x6, #16]
	ldr x5, [x6, #32]

	msr ttbr0_el2, x1
	msr vtcr_el2, x2
	msr mair_el2, x3
	msr tcr_el2, x4

	/* Ensure everything before this point has completed. */
	dsb sy
	isb

	/*
	 * Configure sctlr_el2 to enable MMU and cache and don't proceed until
	 * this has completed.
	 */
	msr sctlr_el2, x5
	isb
	ret
