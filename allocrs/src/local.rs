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

const MAX_SLOTS: usize = 4096;
const BITS_PER_WORD: usize = 64;
const BITMAP_WORDS: usize = MAX_SLOTS / BITS_PER_WORD;

struct Slot<const S: usize> {
    bytes: UnsafeCell<[u8; S]>,
    generation: u32,
    allocated: bool,
    retired: bool,
}

impl<const S: usize> Slot<S> {
    const fn new() -> Self {
        Self {
            bytes: UnsafeCell::new([0; S]),
            generation: 1,
            allocated: false,
            retired: false,
        }
    }
}

/// Single-owner fixed-capacity arena.
///
/// This arena is intentionally not `Send` or `Sync`.
///
/// No atomic operations are required because ownership of the arena is
/// exclusive. This makes it suitable for a dedicated execution core/thread.
#[repr(C, align(64))]
pub struct AllocRSLocal<const S: usize, const N: usize> {
    slots: [Slot<S>; N],
    free_words: [u64; BITMAP_WORDS],
    free_summary: u64,
    allocated: usize,
    retired: usize,
    _not_send_sync: PhantomData<*mut ()>,
}

impl<const S: usize, const N: usize> AllocRSLocal<S, N> {
    pub const fn new() -> Self {
        if S == 0 || S % 64 != 0 {
            panic!("AllocRSLocal: S must be non-zero and divisible by 64");
        }

        if N == 0 || N > MAX_SLOTS {
            panic!("AllocRSLocal: N must be in 1..=4096");
        }

        let slots = [const { Slot::new() }; N];

        let mut free_words = [0u64; BITMAP_WORDS];

        let mut i = 0;
        while i < BITMAP_WORDS {
            let first_slot = i * BITS_PER_WORD;

            if first_slot >= N {
                free_words[i] = 0;
            } else {
                let remaining = N - first_slot;

                free_words[i] = if remaining >= BITS_PER_WORD {
                    u64::MAX
                } else {
                    (1u64 << remaining) - 1
                };
            }

            i += 1;
        }

        let used_words = (N + BITS_PER_WORD - 1) / BITS_PER_WORD;

        let summary = if used_words >= 64 {
            u64::MAX
        } else {
            (1u64 << used_words) - 1
        };

        Self {
            slots,
            free_words,
            free_summary: summary,
            allocated: 0,
            retired: 0,
            _not_send_sync: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        N
    }

    #[inline(always)]
    pub const fn slot_size(&self) -> usize {
        S
    }

    #[inline(always)]
    pub fn allocated_slots(&self) -> usize {
        self.allocated
    }

    #[inline(always)]
    pub fn retired_slots(&self) -> usize {
        self.retired
    }

    #[inline(always)]
    pub fn free_slots(&self) -> usize {
        N - self.allocated - self.retired
    }

    #[inline]
    pub fn allocate(
        &mut self,
    ) -> Result<crate::LocalLease<'_, S, N>, AllocError> {
        if self.free_summary == 0 {
            return Err(AllocError::Full);
        }

        let word_index = self.free_summary.trailing_zeros() as usize;
        let bits = self.free_words[word_index];

        if bits == 0 {
            self.free_summary &= !(1u64 << word_index);
            return self.allocate();
        }

        let bit_index = bits.trailing_zeros() as usize;
        let index = word_index * BITS_PER_WORD + bit_index;

        if index >= N {
            self.free_words[word_index] &= !(1u64 << bit_index);
            return self.allocate();
        }

        let slot = &mut self.slots[index];

        if slot.retired {
            self.free_words[word_index] &= !(1u64 << bit_index);
            return self.allocate();
        }

        let generation = slot.generation;

        self.free_words[word_index] &= !(1u64 << bit_index);

        if self.free_words[word_index] == 0 {
            self.free_summary &= !(1u64 << word_index);
        }

        slot.allocated = true;
        self.allocated += 1;

        Ok(crate::LocalLease {
            arena: self,
            handle: Handle::new(index, generation),
        })
    }

    #[inline]
    pub(crate) fn release(
        &mut self,
        handle: Handle,
    ) -> Result<(), AllocError> {
        let index = handle.index();

        if index >= N {
            return Err(AllocError::InvalidHandle);
        }

        let slot = &mut self.slots[index];

        if !slot.allocated {
            return Err(AllocError::DoubleFree);
        }

        if slot.generation != handle.generation() {
            return Err(AllocError::StaleHandle);
        }

        slot.allocated = false;
        self.allocated -= 1;

        if slot.generation == u32::MAX {
            slot.retired = true;
            self.retired += 1;

            // The slot is deliberately NOT returned to the free bitmap.
            return Err(AllocError::GenerationExhausted);
        }

        slot.generation += 1;

        let word_index = index / BITS_PER_WORD;
        let bit_index = index % BITS_PER_WORD;
        let bit = 1u64 << bit_index;

        self.free_words[word_index] |= bit;
        self.free_summary |= 1u64 << word_index;

        Ok(())
    }

    #[inline(always)]
    pub(crate) fn ptr(
        &self,
        handle: Handle,
    ) -> Result<*mut u8, AllocError> {
        let index = self.validate(handle)?;

        Ok(self.slots[index].bytes.get().cast::<u8>())
    }

    #[inline(always)]
    fn validate(
        &self,
        handle: Handle,
    ) -> Result<usize, AllocError> {
        let index = handle.index();

        if index >= N {
            return Err(AllocError::InvalidHandle);
        }

        let slot = &self.slots[index];

        if !slot.allocated || slot.generation != handle.generation() {
            return Err(AllocError::StaleHandle);
        }

        Ok(index)
    }
}