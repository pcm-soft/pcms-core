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

use core::{cell::UnsafeCell, marker::PhantomData};
use crate::{AllocError, Handle};

const fn words(n: usize) -> usize {
    (n + 63) / 64
}
struct Slot<const S: usize> {
    bytes: UnsafeCell<[u8; S]>,
    generation: u32,
    allocated: bool,
    retired: bool
}
impl<const S: usize> Slot<S> {
    const fn new() -> Self {
        Self {
            bytes: UnsafeCell::new([0; S]),
            generation: 1,
            allocated: false,
            retired: false
        }
    }
}

/// Single-owner arena. It is intentionally not Send/Sync and performs no
/// atomics on the hot path. One owner should keep one arena per execution core.
#[repr(C, align(64))]
pub struct AllocRSLocal<const S: usize, const N: usize> {
    slots: [Slot<S>; N],
    free_words: [u64; words(N)],
    free_summary: u64,
    allocated: usize,
    retired: usize,
    _not_send_sync: PhantomData<*mut ()>,
}
impl<const S: usize, const N: usize> AllocRSLocal<S, N> {
    pub const fn new() -> Self {
        if S == 0 || S % 64 != 0 {
            panic!("S must be non-zero and divisible by 64");
        }
        if N == 0 || N > 4096 {
            panic!("N must be 1..=4096");
        }
        let slots = [const { Slot::new() }; N];
        let mut fw = [0u64; words(N)];
        let mut i = 0;
        while i < fw.len() {
            let rem = N - i * 64;
            fw[i] = if rem >= 64 {
                u64::MAX
            } else {
                (1u64 << rem) - 1 }; i += 1;
        }
        let summary = if words(N) >= 64 {
            u64::MAX
        } else {
            (1u64 << words(N)) - 1
        };
        Self {
            slots,
            free_words: fw,
            free_summary: summary,
            allocated: 0,
            retired: 0,
            _not_send_sync: PhantomData
        }
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize { N }
    #[inline(always)]
    pub const fn slot_size(&self) -> usize { S }
    #[inline(always)]
    pub fn allocated_slots(&self) -> usize { self.allocated }
    #[inline(always)]
    pub fn retired_slots(&self) -> usize { self.retired }
    #[inline(always)]
    pub fn free_slots(&self) -> usize { N - self.allocated - self.retired }

    #[inline]
    pub fn allocate(&mut self) -> Result<crate::LocalLease<'_, S, N>, AllocError> {
        if self.free_summary == 0 {
            return Err(AllocError::Full);
        }
        let wi = self.free_summary.trailing_zeros() as usize;
        let bits = self.free_words[wi];
        if bits == 0 {
            self.free_summary &= !(1u64 << wi);
            return self.allocate();
        }
        let bi = bits.trailing_zeros() as usize;
        let i = wi * 64 + bi;
        self.free_words[wi] &= !(1u64 << bi);
        if self.free_words[wi] == 0 {
            self.free_summary &= !(1u64 << wi);
        }
        self.slots[i].allocated = true; self.allocated += 1;
        Ok(crate::LocalLease {
            arena: self,
            handle: Handle::new(i, self.slots[i].generation)
        })
    }

    #[inline] pub(crate) fn release(&mut self, h: Handle) -> Result<(), AllocError> {
        let i = h.index();
        if i >= N {
            return Err(AllocError::InvalidHandle);
        }
        let s = &mut self.slots[i];
        if !s.allocated {
            return Err(AllocError::DoubleFree);
        }
        if s.generation != h.generation() {
            return Err(AllocError::StaleHandle);
        }
        if s.generation == u32::MAX {
            s.allocated = false;
            s.retired = true;
            self.allocated -= 1;
            self.retired += 1;
            return Err(AllocError::GenerationExhausted);
        }
        s.generation += 1;
        s.allocated = false;
        self.allocated -= 1;
        let wi = i / 64;
        let bi = i % 64;
        self.free_words[wi] |= 1u64 << bi;
        self.free_summary |= 1u64 << wi;
        Ok(())
    }
    #[inline] pub(crate) fn ptr(&self, h: Handle) -> Result<*mut u8, AllocError> {
        let i = self.validate(h)?;
        Ok(self.slots[i].bytes.get())
    }
    #[inline] fn validate(&self, h: Handle) -> Result<usize, AllocError> {
        let i = h.index();
        if i >= N {
            return Err(AllocError::InvalidHandle);
        }
        let s = &self.slots[i];
        if !s.allocated || s.generation != h.generation() {
            return Err(AllocError::StaleHandle);
        } Ok(i)
    }
}
