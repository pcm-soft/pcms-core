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

//! # PCMS memory layout model
//!
//! This module defines the fundamental memory-layout contract used by the
//! PCMS allocator.
//!
//! `Layout` describes:
//!
//! - requested object size;
//! - required alignment;
//! - padded object size;
//! - object stride;
//! - checked and unchecked address calculations with explicit
//!   overflow contracts.
//!
//! This module does not allocate memory and does not access memory.
//!
//! ## Invariants
//!
//! A valid layout guarantees:
//!
//! 1. `size > 0`;
//! 2. `alignment > 0`;
//! 3. `alignment` is a power of two;
//! 4. derived object sizes are validated for overflow;
//! 5. checked address operations detect address overflow;
//! 6. unchecked address operations require the caller to establish
//!    their overflow preconditions.
//!
//! ## Performance
//!
//! Layout calculations are constant-time and use integer arithmetic only.
//! No heap allocation, locking, atomics or system calls are performed.
//!
//! ## Safety
//!
//! This module does not dereference raw pointers.
//! Address calculations return integers and are checked for overflow.

/// Error returned when constructing or manipulating a [`Layout`].
///
/// Layout errors are deterministic and contain no allocator state.
/// They therefore can be handled without allocation or synchronization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutError {
    /// The requested allocation has zero size.
    ///
    /// The PCMS allocator does not represent zero-sized allocations as
    /// ordinary memory blocks. Rejecting them keeps the allocation contract
    /// explicit and prevents special zero-size cases from leaking into
    /// the allocator's hot path.
    ZeroSize,

    /// The requested alignment is invalid.
    ///
    /// Alignment must be non-zero and a power of two.
    InvalidAlignment,

    /// An arithmetic operation used to construct the layout overflowed.
    ///
    /// No partially calculated layout is returned when this happens.
    SizeOverflow,

    /// An address calculation would exceed the representable address space.
    ///
    /// This is kept separate from [`LayoutError::SizeOverflow`] because
    /// size arithmetic and address arithmetic are different validation
    /// boundaries.
    AddressOverflow,
}

/// Describes the memory requirements of an allocation.
///
/// `Layout` does not own memory and does not perform allocation. It is a
/// compact, copyable description that can be passed through allocator,
/// scheduler and subsystem APIs without heap allocation.
///
/// # Invariants
///
/// Every `Layout` constructed through the public constructors is guaranteed
/// to have:
///
/// - a non-zero `size`;
/// - a non-zero power-of-two `align`;
/// - no overflow in the calculations used to construct it.
///
/// The fields are private intentionally. This prevents callers from creating
/// an invalid layout by directly modifying `size` or `align`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    /// Number of bytes required by the allocation.
    size: usize,

    /// Required byte alignment.
    ///
    /// This value is always a non-zero power of two for a valid layout.
    align: usize,

    /// `align - 1`.
    ///
    /// Cached because alignment arithmetic is used on allocator hot paths.
    /// For a valid layout this value is always correct.
    align_mask: usize,
}

impl Layout {
    /// Creates a layout
    ///
    /// `size` — the desired object size in bytes.
    ///
    /// `align` — the desired alignment in bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ZeroSize`] if `size == 0`.
    ///
    /// Returns [`LayoutError::InvalidAlignment`] if `align == 0`
    /// or `align` is not a power of two.
    ///
    /// Returns [`LayoutError::SizeOverflow`] if rounding `size` up to
    /// `align` would overflow `usize`.
    pub const fn new(size: usize, align: usize) -> Result<Self, LayoutError> {
        if size == 0 {
            return Err(LayoutError::ZeroSize);
        }

        if align == 0 || !align.is_power_of_two() {
            return Err(LayoutError::InvalidAlignment);
        }

        let align_mask = align - 1;

        if size.checked_add(align_mask).is_none() {
            return Err(LayoutError::SizeOverflow);
        }

        Ok(Self {
            size, 
            align,
            align_mask,
        })
    }

    /// Returns the requested object size in bytes.
    #[inline(always)]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Returns the required byte alignment.
    #[inline(always)]
    pub const fn align(&self) -> usize {
        self.align
    }

    /// Returns the cached alignment mask.
    ///
    /// For every valid layout:
    ///
    /// `mask == align - 1`
    #[inline(always)]
    pub const fn align_mask(&self) -> usize {
        self.align_mask
    }

    /// Returns the number of bytes required to pad `address` to the next
    /// address satisfying this layout's alignment.
    ///
    /// The result is in the range `0..align`.
    ///
    /// Because the alignment is guaranteed to be a power of two, the
    /// calculation reduces to:
    ///
    /// `(-address) & (align - 1)`
    #[inline(always)]
    pub const fn padding_for(&self, address: usize) -> usize {
        address.wrapping_neg() & self.align_mask
    }

    /// Returns the requested size rounded up to the required alignment.
    ///
    /// The result is guaranteed to be a multiple of `align`.
    ///
    /// The overflow condition is checked during [`Layout::new`], therefore
    /// this operation cannot overflow for a valid layout.
    #[inline(always)]
    pub const fn padded_size(&self) -> usize {
        (self.size + self.align_mask) & !self.align_mask
    }

    /// Returns the byte distance between the starts of two consecutive
    /// objects having this layout.
    ///
    /// For the PCMS allocator, every object begins at an address satisfying
    /// `align`, therefore the object stride is the padded size.
    #[inline(always)]
    pub const fn stride(&self) -> usize {
        self.padded_size()
    }

    /// Checks whether an address satisfies this layout's alignment.
    #[inline(always)]
    pub const fn is_aligned(&self, address: usize) -> bool {
        (address & self.align_mask) == 0
    }

    /// Aligns an address upward to this layout's alignment.
    ///
    /// Returns [`LayoutError::AddressOverflow`] when rounding the address
    /// upward would exceed `usize::MAX`.
    #[inline(always)]
    pub const fn align_up(&self, address: usize) -> Result<usize, LayoutError> {
        let padding = self.padding_for(address);

        match address.checked_add(padding) {
            Some(aligned) => Ok(aligned),
            None => Err(LayoutError::AddressOverflow),
        }
    }

    /// Aligns an address upward to this layout's alignment without checking
    /// for address overflow.
    ///
    /// # Preconditions
    ///
    /// The caller must guarantee:
    ///
    /// `address + self.padding_for(address) <= usize::MAX`.
    ///
    /// This function performs no overflow check and is intended for allocator
    /// hot paths where the address range has already been validated.
    ///
    /// Use [`Self::align_up`] when this precondition cannot be guaranteed.
    #[inline(always)]
    pub const fn align_up_unchecked(&self, address: usize) -> usize {
        address + self.padding_for(address)
    }

    /// Returns the exclusive end address of the padded object.
    ///
    /// This function checks that adding the padded object size to `address`
    /// does not overflow `usize`.
    ///
    /// The returned value is the first address immediately after the padded
    /// object.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::AddressOverflow`] if the end address cannot be
    /// represented by `usize`.
    #[inline(always)]
    pub const fn checked_end(
      &self,
      address: usize,
    ) -> Result<usize, LayoutError> {
     match address.checked_add(self.padded_size()) {
           Some(end) => Ok(end),
           None => Err(LayoutError::AddressOverflow),
     }
    }

    /// Returns the exclusive end address of the padded object without checking
    /// for address overflow.
    ///
    /// The object occupies the half-open range:
    ///
    /// `[address, address + self.padded_size())`.
    ///
    /// # Preconditions
    ///
    /// The caller must guarantee:
    ///
    /// `address + self.padded_size() <= usize::MAX`.
    ///
    /// This function performs no overflow check and is intended for allocator
    /// hot paths where the address range has already been validated.
    ///
    /// Use [`Self::checked_end`] when the address range has not already been
    /// validated.
    #[inline(always)]
    pub const fn end_unchecked(&self, address: usize) -> usize {
        address + self.padded_size()
    }

    /// Checks whether the complete padded object fits within a memory region.
    ///
    /// The memory region is represented as a half-open range:
    ///
    /// `[.., region_end_exclusive)`
    ///
    /// The object occupies:
    ///
    /// `[address, address + padded_size)`
    ///
    /// Returns `true` exactly when the complete object is representable and
    /// its exclusive end does not exceed `region_end_exclusive`.
    ///
    /// Address overflow is treated as failure.
    #[inline(always)]
    pub const fn contains_range(
        &self,
        address: usize,
        region_end_exclusive: usize,
    ) -> bool {
        match address.checked_add(self.padded_size()) {
            Some(end) => end <= region_end_exclusive,
            None => false,
        }
    }
}
