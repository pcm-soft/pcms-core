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

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    sync::atomic::{
        AtomicU64,
        AtomicUsize,
        Ordering,
    },
};

use crate::{AllocError, Handle};

const MAX_SLOTS: usize = 4096;
const BITS_PER_WORD: usize = 64;
const BITMAP_WORDS: usize = MAX_SLOTS / BITS_PER_WORD;

/// Atomic state of one allocation slot.
///
/// Bit 0:
///     allocation state
///
/// Bits 1..32:
///     generation counter
///
/// The remaining bits are unused.
#[repr(transparent)]
struct SlotState(AtomicU64);

impl SlotState {
    const fn new() -> Self {
        // generation = 1, allocated = false
        Self(AtomicU64::new(2))
    }

    #[inline(always)]
    fn generation(value: u64) -> u32 {
        (value >> 1) as u32
    }

    #[inline(always)]
    fn allocated(value: u64) -> bool {
        value & 1 != 0
    }

    /// Attempts to transition the slot from free to allocated.
    ///
    /// Returns the generation associated with the allocation.
    #[inline]
    fn try_allocate(&self) -> Result<u32, ()> {
        let mut current = self.0.load(Ordering::Acquire);

        loop {
            if Self::allocated(current) {
                return Err(());
            }

            let generation = Self::generation(current);

            // A retired generation must never be allocated again.
            if generation == u32::MAX {
                return Err(());
            }

            let next =
                ((generation as u64) << 1) | 1;

            match self.0.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(generation),
                Err(observed) => current = observed,
            }
        }
    }

    /// Attempts to release a live allocation.
    #[inline]
    fn try_free(
        &self,
        expected_generation: u32,
    ) -> Result<(), AllocError> {
        let current = self.0.load(Ordering::Acquire);

        if !Self::allocated(current) {
            return Err(AllocError::DoubleFree);
        }

        let generation = Self::generation(current);

        if generation != expected_generation {
            return Err(AllocError::StaleHandle);
        }

        // The final valid generation is retired rather than wrapped.
        let next = if expected_generation == u32::MAX {
            // Keep the generation at MAX and clear allocation state.
            (u32::MAX as u64) << 1
        } else {
            ((expected_generation as u64 + 1) << 1)
        };

        match self.0.compare_exchange(
            current,
            next,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                if expected_generation == u32::MAX {
                    Err(AllocError::GenerationExhausted)
                } else {
                    Ok(())
                }
            }

            Err(observed) => {
                if !Self::allocated(observed) {
                    Err(AllocError::DoubleFree)
                } else {
                    Err(AllocError::StaleHandle)
                }
            }
        }
    }
}

/// Creates the statically sized slot-storage array.
///
/// The array length is `N`, which is a standalone const generic and is
/// therefore supported without `generic_const_exprs`.
const fn make_storage<const S: usize, const N: usize>()
    -> [UnsafeCell<[u8; S]>; N]
{
    let mut storage =
        MaybeUninit::<[UnsafeCell<[u8; S]>; N]>::uninit();

    let base =
        storage.as_mut_ptr() as *mut UnsafeCell<[u8; S]>;

    let mut i = 0;

    while i < N {
        // SAFETY:
        //
        // `base` points to the first element of an uninitialized array.
        // `i < N` guarantees that this element is inside the array.
        // Every element is initialized exactly once before assume_init().
        unsafe {
            base.add(i)
                .write(UnsafeCell::new([0u8; S]));
        }

        i += 1;
    }

    // SAFETY:
    //
    // The loop above initialized every one of the N elements.
    unsafe { storage.assume_init() }
}

/// Creates the slot-state array.
const fn make_slots<const N: usize>() -> [SlotState; N] {
    [const { SlotState::new() }; N]
}

/// Shared lock-free fixed-capacity arena.
///
/// The arena contains at most 4096 slots. The bitmap itself is statically
/// bounded to 64 words, avoiding unstable generic constant expressions.
///
/// Allocation uses a two-level bitmap:
///
/// `free_summary`
///     identifies bitmap words containing free slots.
///
/// `free_words[i]`
///     identifies individual free slots.
///
/// The common allocation path therefore requires only a bounded number of
/// atomic operations.
#[repr(C, align(64))]
pub struct AllocRS<const S: usize, const N: usize> {
    storage: [UnsafeCell<[u8; S]>; N],
    slots: [SlotState; N],

    /// One bit per allocation slot.
    free_words: [AtomicU64; BITMAP_WORDS],

    /// One bit per bitmap word.
    free_summary: AtomicU64,

    allocated: AtomicUsize,
    retired: AtomicUsize,

    _not_send_sync: PhantomData<*mut ()>,
}

impl<const S: usize, const N: usize> AllocRS<S, N> {
    /// Creates a statically allocated shared arena.
    pub const fn new() -> Self {
        if S == 0 || S % 64 != 0 {
            panic!("AllocRS: S must be non-zero and divisible by 64");
        }

        if N == 0 || N > MAX_SLOTS {
            panic!("AllocRS: N must be in 1..=4096");
        }

        let storage =
            make_storage::<S, N>();

        let slots =
            make_slots::<N>();

        let mut free_words =
            [const { AtomicU64::new(0) }; BITMAP_WORDS];

        let mut i = 0;

        while i < BITMAP_WORDS {
            let first_slot =
                i * BITS_PER_WORD;

            if first_slot < N {
                let remaining =
                    N - first_slot;

                let value =
                    if remaining >= BITS_PER_WORD {
                        u64::MAX
                    } else {
                        (1u64 << remaining) - 1
                    };

                free_words[i] =
                    AtomicU64::new(value);
            }

            i += 1;
        }

        let used_words =
            (N + BITS_PER_WORD - 1) / BITS_PER_WORD;

        let summary =
            if used_words >= 64 {
                u64::MAX
            } else {
                (1u64 << used_words) - 1
            };

        Self {
            storage,
            slots,
            free_words,
            free_summary: AtomicU64::new(summary),
            allocated: AtomicUsize::new(0),
            retired: AtomicUsize::new(0),
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
        self.allocated.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn retired_slots(&self) -> usize {
        self.retired.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn free_slots(&self) -> usize {
        N - self.allocated_slots() - self.retired_slots()
    }

    /// Allocates one slot from the shared arena.
    ///
    /// The operation is lock-free with respect to other allocators. It does
    /// not allocate memory from the heap.
    #[inline]
    pub fn allocate(
        &self,
    ) -> Result<crate::Lease<'_, S, N>, AllocError> {
        loop {
            let summary =
                self.free_summary.load(Ordering::Acquire);

            if summary == 0 {
                return Err(AllocError::Full);
            }

            let word_index =
                summary.trailing_zeros() as usize;

            let word =
                &self.free_words[word_index];

            let mut bits =
                word.load(Ordering::Acquire);

            while bits != 0 {
                let bit_index =
                    bits.trailing_zeros() as usize;

                let mask =
                    1u64 << bit_index;

                bits &= !mask;

                let index =
                    word_index * BITS_PER_WORD +
                    bit_index;

                if index >= N {
                    continue;
                }

                let slot =
                    &self.slots[index];

                if let Ok(generation) =
                    slot.try_allocate()
                {
                    // Claim the bitmap bit after successfully claiming the
                    // slot state.
                    //
                    // Other allocators may race for the same bit. The slot
                    // state remains the authoritative ownership check.
                    let old =
                        word.fetch_and(
                            !mask,
                            Ordering::AcqRel,
                        );

                    if old & mask != 0 {
                        if word.load(Ordering::Acquire) == 0 {
                            self.free_summary.fetch_and(
                                !(1u64 << word_index),
                                Ordering::AcqRel,
                            );
                        }
                    }

                    self.allocated.fetch_add(
                        1,
                        Ordering::Relaxed,
                    );

                    return Ok(crate::Lease {
                        arena: self,
                        handle: Handle::new(
                            index,
                            generation,
                        ),
                    });
                }
            }

            // The summary can become stale because another thread may have
            // claimed the last slot in this word.
            if word.load(Ordering::Acquire) == 0 {
                self.free_summary.fetch_and(
                    !(1u64 << word_index),
                    Ordering::AcqRel,
                );
            }
        }
    }

    /// Releases a live allocation.
    #[inline]
    pub(crate) fn release(
        &self,
        handle: Handle,
    ) -> Result<(), AllocError> {
        let index =
            handle.index();

        if index >= N {
            return Err(AllocError::InvalidHandle);
        }

        match self.slots[index]
            .try_free(handle.generation())
        {
            Ok(()) => {
                let word_index =
                    index / BITS_PER_WORD;

                let bit_index =
                    index % BITS_PER_WORD;

                let mask =
                    1u64 << bit_index;

                self.free_words[word_index]
                    .fetch_or(
                        mask,
                        Ordering::Release,
                    );

                self.free_summary.fetch_or(
                    1u64 << word_index,
                    Ordering::Release,
                );

                self.allocated.fetch_sub(
                    1,
                    Ordering::Relaxed,
                );

                Ok(())
            }

            Err(AllocError::GenerationExhausted) => {
                self.allocated.fetch_sub(
                    1,
                    Ordering::Relaxed,
                );

                self.retired.fetch_add(
                    1,
                    Ordering::Relaxed,
                );

                Err(
                    AllocError::GenerationExhausted
                )
            }

            Err(error) => Err(error),
        }
    }

    /// Returns the raw pointer for a currently valid handle.
    #[inline(always)]
    pub(crate) fn ptr(
        &self,
        handle: Handle,
    ) -> Result<*mut u8, AllocError> {
        let index =
            self.validate(handle)?;

        Ok(
            self.storage[index]
                .get()
                .cast::<u8>()
        )
    }

    #[inline(always)]
    fn validate(
        &self,
        handle: Handle,
    ) -> Result<usize, AllocError> {
        let index =
            handle.index();

        if index >= N {
            return Err(AllocError::InvalidHandle);
        }

        let state =
            self.slots[index]
                .0
                .load(Ordering::Acquire);

        if !SlotState::allocated(state) ||
            SlotState::generation(state)
                != handle.generation()
        {
            return Err(AllocError::StaleHandle);
        }

        Ok(index)
    }

    /// Allocates a slot and returns the raw handle.
    ///
    /// # Safety
    ///
    /// The caller becomes responsible for releasing the returned handle
    /// exactly once with `free_raw()`. The caller must not access the
    /// allocation after it has been released.
    pub unsafe fn allocate_raw(
        &self,
    ) -> Result<Handle, AllocError> {
        let lease =
            self.allocate()?;

        let handle =
            lease.handle;

        core::mem::forget(lease);

        Ok(handle)
    }

    /// Releases a raw allocation handle.
    ///
    /// # Safety
    ///
    /// The caller must own the handle and must guarantee that it is released
    /// exactly once.
    #[inline]
    pub unsafe fn free_raw(
        &self,
        handle: Handle,
    ) -> Result<(), AllocError> {
        self.release(handle)
    }
}

/// `AllocRS` contains internally synchronized atomic state and immutable slot
/// storage whose mutable access is controlled by validated handles.
unsafe impl<const S: usize, const N: usize> Send
    for AllocRS<S, N>
{
}

unsafe impl<const S: usize, const N: usize> Sync
    for AllocRS<S, N>
{
}