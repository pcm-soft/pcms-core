// Copyright 2026 Gleb Obitotsky
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#[inline(always)]
pub fn read_tsc_cycles() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY:
        // `_rdtsc` emits the RDTSC instruction and does not dereference
        // memory or access invalid CPU state.
        unsafe {
            core::arch::x86_64::_rdtsc()
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        let value: u64;

        // SAFETY:
        // CNTVCT_EL0 is the architectural virtual counter on AArch64.
        unsafe {
            core::arch::asm!(
            "mrs {0}, cntvct_el0",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
            );
        }

        value
    }

    #[cfg(target_arch = "riscv64")]
    {
        let value: u64;

        // SAFETY:
        // `rdtime` reads the architectural time counter.
        unsafe {
            core::arch::asm!(
            "rdtime {0}",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
            );
        }

        value
    }
}

#[inline(always)]
pub fn spin_for_cycles(cycles: u64) {
    let start = read_tsc_cycles();

    while read_tsc_cycles().wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}

#[inline(always)]
pub fn deadline_expired(start: u64, limit: u64) -> bool {
    read_tsc_cycles().wrapping_sub(start) >= limit
}