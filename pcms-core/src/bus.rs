// Copyright 2026 Gleb Obitotsky
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Cache-line alignment used for synchronization objects.
///
/// 64 bytes is the common cache-line size on the architectures targeted
/// by the initial PCMS implementation. It is deliberately represented as
/// a type rather than applying `repr(align)` to an atomic field.
const CACHE_LINE: usize = 64;

/// A cache-line-aligned atomic value.
///
/// Keeping producer and consumer counters in separately aligned objects
/// prevents unnecessary cache-line sharing between the two sides.
#[repr(align(64))]
struct AlignedAtomicUsize(AtomicUsize);

impl AlignedAtomicUsize {
    #[inline(always)]
    const fn new(value: usize) -> Self {
        Self(AtomicUsize::new(value))
    }

    #[inline(always)]
    fn load(&self, ordering: Ordering) -> usize {
        self.0.load(ordering)
    }

    #[inline(always)]
    fn compare_exchange_weak(
        &self,
        current: usize,
        new: usize,
        success: Ordering,
        failure: Ordering,
    ) -> Result<usize, usize> {
        self.0
            .compare_exchange_weak(current, new, success, failure)
    }
}

#[repr(C, align(64))]
struct QueueSlot<T> {
    sequence: AtomicUsize,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> QueueSlot<T> {
    const EMPTY: Self = Self {
        sequence: AtomicUsize::new(0),
        value: UnsafeCell::new(MaybeUninit::uninit()),
    };

    #[inline(always)]
    const fn new(sequence: usize) -> Self {
        Self {
            sequence: AtomicUsize::new(sequence),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

/// Errors returned by the realtime bus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusError {
    /// The queue has no currently available slot.
    ///
    /// This operation does not wait.
    Full,

    /// The queue is empty.
    Empty,

    /// The queue capacity is invalid.
    ///
    /// This variant is primarily useful for APIs where construction cannot
    /// fail at compile time.
    InvalidCapacity,
}

/// Fixed-capacity lock-free MPSC queue.
///
/// `N` must be a power of two and at least 2.
///
/// The queue itself does not allocate memory.
///
/// # Example
///
/// ```ignore
/// let queue = PcmsBus::<MedicalPacket, 256>::new();
///
/// queue.push(packet)?;
///
/// let packet = queue.pop()?;
/// # Ok::<(), BusError>(())
/// ```
#[repr(C, align(64))]
pub struct PcmsBus<T, const N: usize> {
    /// Producer reservation position.
    ///
    /// Only producers modify this counter.
    enqueue_pos: AlignedAtomicUsize,

    /// Consumer position.
    ///
    /// Only the single consumer modifies this counter.
    dequeue_pos: AlignedAtomicUsize,

    /// Fixed slot storage.
    slots: [QueueSlot<T>; N],
}

unsafe impl<T: Send, const N: usize> Send for PcmsBus<T, N> {}

unsafe impl<T: Send, const N: usize> Sync for PcmsBus<T, N> {}

impl<T, const N: usize> PcmsBus<T, N> {
    #[inline]
    pub const fn new() -> Self {
        assert!(N >= 2);
        assert!(N.is_power_of_two());

        Self {
            enqueue_pos: AlignedAtomicUsize::new(0),
            dequeue_pos: AlignedAtomicUsize::new(0),
            slots: Self::make_slots(),
        }
    }

    const fn make_slots() -> [QueueSlot<T>; N] {
        let mut slots = [QueueSlot::EMPTY; N];
        let mut i = 0;
        while i < N {
            slots[i] = QueueSlot::new(i);
            i += 1;
        }
        slots
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        N
    }

    #[inline]
    pub fn push(&self, value: T) -> Result<(), BusError> {
        let mask = N - 1;
        let mut position = self.enqueue_pos.load(Ordering::Relaxed);

        loop {
            let slot_index = position & mask;
            let slot = &self.slots[slot_index];

            let sequence = slot.sequence.load(Ordering::Acquire);

            let difference = sequence.wrapping_sub(position);

            if difference == 0 {
                match self.enqueue_pos.compare_exchange_weak(
                    position,
                    position.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        unsafe {
                            (*slot.value.get()).write(value);
                        }

                        slot.sequence
                            .store(position.wrapping_add(1), Ordering::Release);

                        return Ok(());
                    }
                    Err(next_position) => {
                        position = next_position;
                    }
                }
            } else if difference > 0 {
                position = self.enqueue_pos.load(Ordering::Relaxed);
            } else {
                return Err(BusError::Full);
            }
        }
    }

    #[inline]
    pub fn pop(&self) -> Result<T, BusError> {
        let mask = N - 1;
        let position = self.dequeue_pos.load(Ordering::Relaxed);

        let slot_index = position & mask;
        let slot = &self.slots[slot_index];

        let sequence = slot.sequence.load(Ordering::Acquire);

        let expected = position.wrapping_add(1);
        let difference = sequence.wrapping_sub(expected);

        if difference == 0 {
            self.dequeue_pos
                .0
                .store(position.wrapping_add(1), Ordering::Relaxed);

            let value = unsafe { (*slot.value.get()).assume_init_read() };

            slot.sequence
                .store(position.wrapping_add(N), Ordering::Release);

            return Ok(value);
        }

        if difference < 0 {
            return Err(BusError::Empty);
        }

        Err(BusError::Empty)
    }

    #[inline]
    pub fn len_approx(&self) -> usize {
        let head = self.enqueue_pos.load(Ordering::Acquire);
        let tail = self.dequeue_pos.load(Ordering::Acquire);

        head.wrapping_sub(tail).min(N)
    }

    #[inline(always)]
    pub fn is_empty_approx(&self) -> bool {
        self.len_approx() == 0
    }

    #[inline(always)]
    pub fn is_full_approx(&self) -> bool {
        self.len_approx() >= N
    }
}

impl<T, const N: usize> Default for PcmsBus<T, N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for PcmsBus<T, N> {
    fn drop(&mut self) {
        while let Ok(value) = self.pop() {
            drop(value);
        }
    }
}

/// Compile-time assertion helper.
///
/// Kept separate so the cache-line contract is explicit.
const _: () = {
    assert!(CACHE_LINE == 64);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_order() {
        let queue = PcmsBus::<u32, 8>::new();

        assert!(queue.push(10).is_ok());
        assert!(queue.push(20).is_ok());
        assert!(queue.push(30).is_ok());

        assert_eq!(queue.pop(), Ok(10));
        assert_eq!(queue.pop(), Ok(20));
        assert_eq!(queue.pop(), Ok(30));
        assert_eq!(queue.pop(), Err(BusError::Empty));
    }

    #[test]
    fn full_queue_does_not_block() {
        let queue = PcmsBus::<u32, 4>::new();

        assert!(queue.push(1).is_ok());
        assert!(queue.push(2).is_ok());
        assert!(queue.push(3).is_ok());
        assert!(queue.push(4).is_ok());

        assert_eq!(queue.push(5), Err(BusError::Full));
    }

    #[test]
    fn reuse_after_pop() {
        let queue = PcmsBus::<u32, 4>::new();

        for i in 0..1000 {
            assert_eq!(queue.push(i), Ok(()));
            assert_eq!(queue.pop(), Ok(i));
        }
    }

    #[test]
    fn capacity_is_correct() {
        let queue = PcmsBus::<u32, 64>::new();
        assert_eq!(queue.capacity(), 64);
    }
}