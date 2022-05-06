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

        /* Remove our exception vector. */
        msr vbar_el1, xzr

        /* Disable the MMU and cache. */
        msr sctlr_el1, xzr
        msr ttbr0_el1, xzr
        msr mair_el1, xzr
        msr tcr_el1, xzr

        isb

        /* Jump into the payload. */
        br x30
