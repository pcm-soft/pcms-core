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

use core::{cell::UnsafeCell, marker::PhantomData, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
use crate::{AllocError, Handle};

const MAX_SLOTS: usize = 4096;
const fn words(n: usize) -> usize { (n + 63) / 64 }

#[repr(transparent)]
struct SlotState(AtomicU64);
impl SlotState {
    const fn new() -> Self {
        Self(AtomicU64::new(2))
    }
    #[inline(always)]
    fn generation(v: u64) -> u32 { (v >> 1) as u32 }
    #[inline(always)]
    fn allocated(v: u64) -> bool { v & 1 != 0 }
    #[inline]
    fn try_allocate(&self) -> Result<u32, ()> {
        let mut cur = self.0.load(Ordering::Acquire);
        loop {
            if Self::allocated(cur) {
                return Err(());
            }
            let gen = Self::generation(cur);
            let next = ((gen as u64) << 1) | 1;
            match self.0.compare_exchange_weak(cur, next, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => return Ok(gen), Err(v) => cur = v
            }
        }
    }

    #[inline] fn try_free(&self, expected: u32) -> Result<(), AllocError> {
        let cur = self.0.load(Ordering::Acquire);
        if !Self::allocated(cur) {
            return Err(AllocError::DoubleFree);
        }
        if Self::generation(cur) != expected {
            return Err(AllocError::StaleHandle);
        }
        let next = if expected == u32::MAX {
            0
        } else {
            (expected as u64 + 1) << 1
        };
        match self.0.compare_exchange(cur, next, Ordering::Release, Ordering::Acquire) {
            Ok(_) => Ok(()), Err(v) => { if !Self::allocated(v) {
                Err(AllocError::DoubleFree)
            } else {
                Err(AllocError::StaleHandle)
            } }
        }
    }
}

/// Shared lock-free arena. N <= 4096 keeps the two-level bitmap bounded.
/// For larger capacities, shard several arenas rather than making one shared
/// global pool.
#[repr(C, align(64))]
pub struct AllocRS<const S: usize, const N: usize> {
    storage: [UnsafeCell<[u8; S]>; N],
    slots: [SlotState; N],
    free_words: [AtomicU64; words(N)],
    free_summary: AtomicU64,
    allocated: AtomicUsize,
    retired: AtomicUsize,
    _not_send_sync: PhantomData<*mut ()>,
}
impl<const S: usize, const N: usize> AllocRS<S, N> {
    pub const fn new() -> Self {
        if S == 0 || S % 64 != 0 {
            panic!("S must be non-zero and divisible by 64");
        }
        if N == 0 || N > MAX_SLOTS {
            panic!("N must be 1..=4096");
        }
        let storage = [const { UnsafeCell::new([0u8; S]) }; N];
        let slots = [const { SlotState::new() }; N];
        let mut words_arr = [const { AtomicU64::new(0) }; words(N)];
        let mut i = 0;
        while i < words_arr.len() {
            let rem = N - i * 64;
            words_arr[i] = AtomicU64::new(if rem >= 64 { u64::MAX } else { (1u64 << rem) - 1 });
            i += 1;
        }
        let summary = if words(N) >= 64 {
            u64::MAX
        } else {
            (1u64 << words(N)) - 1
        };
        Self {
            storage,
            slots,
            free_words: words_arr,
            free_summary: AtomicU64::new(summary),
            allocated: AtomicUsize::new(0),
            retired: AtomicUsize::new(0),
            _not_send_sync: PhantomData
        }
    }
    #[inline(always)] pub const fn capacity(&self) -> usize { N }
    #[inline(always)] pub const fn slot_size(&self) -> usize { S }
    #[inline(always)] pub fn allocated_slots(&self) -> usize { self.allocated.load(Ordering::Relaxed) }
    #[inline(always)] pub fn retired_slots(&self) -> usize { self.retired.load(Ordering::Relaxed) }
    #[inline(always)] pub fn free_slots(&self) -> usize { N - self.allocated_slots() - self.retired_slots() }

    #[inline]
    pub fn allocate(&self) -> Result<crate::Lease<'_, S, N>, AllocError> {
        loop {
            let summary = self.free_summary.load(Ordering::Acquire);
            if summary == 0 {
                // Summary bits are an accelerator, not the source of truth.
                // A concurrent free can race a summary clear, so recover by
                // scanning the bounded (<=64) word array and rebuilding the
                // missing summary bit.
                let mut recovered = false;
                let mut j = 0;
                while j < self.free_words.len() {
                    if self.free_words[j].load(Ordering::Acquire) != 0 {
                        self.free_summary.fetch_or(1u64 << j, Ordering::AcqRel);
                        recovered = true;
                        break;
                    }
                    j += 1;
                }
                if !recovered {
                    return Err(AllocError::Full);
                }
                continue;
            }
            let wi = summary.trailing_zeros() as usize;
            let word = &self.free_words[wi];
            let mut bits = word.load(Ordering::Acquire);
            while bits != 0 {
                let bi = bits.trailing_zeros() as usize;
                let mask = 1u64 << bi;
                bits &= !mask;
                let index = wi * 64 + bi;
                if index >= N { continue; }
                if let Ok(gen) = self.slots[index].try_allocate() {
                    let old = word.fetch_and(!mask, Ordering::AcqRel);
                    if old & !mask == 0 {
                        // Re-read after clearing the final observed bit. A
                        // concurrent free may already have published a new
                        // free bit; never clear the summary in that case.
                        if word.load(Ordering::Acquire) == 0 {
                            self.free_summary.fetch_and(!(1u64 << wi), Ordering::AcqRel);
                        }
                    }
                    self.allocated.fetch_add(1, Ordering::Relaxed);
                    return Ok(crate::Lease {
                        arena: self, handle: Handle::new(index, gen)
                    });
                }
            }
            if word.load(Ordering::Acquire) == 0 {
                self.free_summary.fetch_and(!(1u64 << wi),
                                            Ordering::AcqRel);
            }
        }
    }

    #[inline] pub(crate) fn release(&self, h: Handle) -> Result<(), AllocError> {
        let i = h.index(); if i >= N {
            return Err(AllocError::InvalidHandle);
        }
        match self.slots[i].try_free(h.generation()) {
            Ok(()) => {
                let wi = i / 64;
                let bi = i % 64;
                self.free_words[wi].fetch_or(1u64 << bi, Ordering::Release);
                self.free_summary.fetch_or(1u64 << wi, Ordering::Release);
                self.allocated.fetch_sub(1, Ordering::Relaxed);
                Ok(())
            }
            Err(AllocError::GenerationExhausted) => {
                self.allocated.fetch_sub(1, Ordering::Relaxed);
                self.retired.fetch_add(1, Ordering::Relaxed);
                Err(AllocError::GenerationExhausted)
            }
            Err(e) => Err(e),
        }
    }
    #[inline] pub(crate) fn ptr(&self, h: Handle) -> Result<*mut u8, AllocError> {
        let i = self.validate(h)?;
        Ok(self.storage[i].get())
    }

    #[inline] fn validate(&self, h: Handle) -> Result<usize, AllocError> {
        let i = h.index();
        if i >= N {
            return Err(AllocError::InvalidHandle);
        }
        let s = self.slots[i].0.load(Ordering::Acquire);
        if !SlotState::allocated(s) || SlotState::generation(s) != h.generation() {
            return Err(AllocError::StaleHandle);
        }
        Ok(i)
    }

    /// Unsafe escape hatch for integration with APIs that require an opaque
    /// token. The caller must return the token exactly once using free_raw and
    /// must not access the slot after release.
    pub unsafe fn allocate_raw(&self) -> Result<Handle, AllocError> {
        let lease = self.allocate()?;
        let h = lease.handle;
        core::mem::forget(lease);
        Ok(h)
    }
    /// Unsafe raw-handle release. Prefer Lease for all safe code.
    pub unsafe fn free_raw(&self, h: Handle) -> Result<(), AllocError> {
        self.release(h)
    }
}
unsafe impl<const S: usize, const N: usize> Send for AllocRS<S, N> {}
unsafe impl<const S: usize, const N: usize> Sync for AllocRS<S, N> {}
