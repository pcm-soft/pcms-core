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

//! # PCMS allocation size classes
//!
//! This module defines [`SizeClass`], the fixed-size allocation unit used by
//! the PCMS allocator.
//!
//! A size class represents one validated allocation geometry:
//!
//! - requested object size;
//! - required alignment;
//! - cached alignment mask;
//! - padded object size;
//! - object stride.
//!
//! `SizeClass` does not reimplement alignment arithmetic or overflow checks.
//! Instead, it builds on an already validated [`Layout`] and exposes the
//! operations required by the allocator's hot paths.
//!
//! The hot-path operations consist only of integer comparisons and arithmetic.

use crate::layout::{Layout, LayoutError};

/// Error returned when constructing a [`SizeClass`].
///
/// The error type is intentionally small and allocation-free.
///
/// Most validation is delegated to [`Layout::new`]. The additional
/// [`SizeClassError::InvalidStride`] variant protects the stronger invariant
/// required by the allocator: every valid size class must have a non-zero
/// stride.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SizeClassError {
    /// The underlying memory layout is invalid.
    Layout(LayoutError),

    /// The resulting padded object size is zero.
    ///
    /// This should not occur for a valid [`Layout`], but it is retained as an
    /// explicit invariant at the `SizeClass` boundary.
    InvalidStride,
}

impl From<LayoutError> for SizeClassError {
    #[inline(always)]
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

/// A validated fixed-size allocation class.
///
/// A `SizeClass` describes the geometry used for allocations belonging to
/// one allocator class.
///
/// The contained [`Layout`] remains the authoritative representation of:
///
/// - requested size;
/// - alignment;
/// - alignment mask.
///
/// `stride` is cached separately because it is one of the allocator's most
/// frequently accessed values. Keeping it directly in the size class avoids
/// repeating the padded-size calculation in allocator hot paths.
///
/// # Invariants
///
/// A `SizeClass` created by [`SizeClass::new`] guarantees:
///
/// - `layout.size() > 0`;
/// - `layout.align() > 0`;
/// - `layout.align()` is a power of two;
/// - `layout.align_mask() == layout.align() - 1`;
/// - `stride > 0`;
/// - `stride == layout.padded_size()`;
/// - `stride % layout.align() == 0`.
///
/// These invariants are established once during construction and are never
/// modified afterwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SizeClass {
    /// Validated memory layout for objects in this class.
    layout: Layout,

    /// Cached padded object size.
    ///
    /// This is the distance between the beginning of consecutive objects
    /// when every object starts at an address satisfying the class
    /// alignment.
    stride: usize,
}

impl SizeClass {
    /// Creates a new validated allocation size class.
    ///
    /// `size` is the requested payload size in bytes.
    ///
    /// `align` is the required alignment in bytes.
    ///
    /// Construction delegates fundamental validation to [`Layout::new`].
    /// The resulting padded size is then cached as the class stride.
    ///
    /// # Errors
    ///
    /// Returns [`SizeClassError::Layout`] when the requested layout is
    /// invalid or its arithmetic overflows.
    ///
    /// Returns [`SizeClassError::InvalidStride`] if the resulting padded
    /// size is zero.
    #[inline]
    pub const fn new(
        size: usize,
        align: usize,
    ) -> Result<Self, SizeClassError> {
        let layout = match Layout::new(size, align) {
            Ok(layout) => layout,
            Err(error) => return Err(SizeClassError::Layout(error)),
        };

        let stride = layout.padded_size();

        if stride == 0 {
            return Err(SizeClassError::InvalidStride);
        }

        Ok(Self { layout, stride })
    }

    /// Creates a size class directly from an already validated layout.
    ///
    /// This constructor is useful when the allocator already owns a
    /// [`Layout`] and should not reconstruct it.
    ///
    /// No layout validation is repeated because the `Layout` type itself
    /// guarantees its invariants.
    ///
    /// # Errors
    ///
    /// Returns [`SizeClassError::InvalidStride`] if the layout produces a
    /// zero stride.
    #[inline]
    pub const fn from_layout(
        layout: Layout,
    ) -> Result<Self, SizeClassError> {
        let stride = layout.padded_size();

        if stride == 0 {
            return Err(SizeClassError::InvalidStride);
        }

        Ok(Self { layout, stride })
    }

    /// Returns the underlying validated [`Layout`].
    #[inline(always)]
    pub const fn layout(&self) -> Layout {
        self.layout
    }

    /// Returns the requested payload size in bytes.
    #[inline(always)]
    pub const fn size(&self) -> usize {
        self.layout.size()
    }

    /// Returns the required alignment in bytes.
    #[inline(always)]
    pub const fn align(&self) -> usize {
        self.layout.align()
    }

    /// Returns the cached alignment mask.
    ///
    /// For every valid class:
    ///
    /// `align_mask == align - 1`
    #[inline(always)]
    pub const fn align_mask(&self) -> usize {
        self.layout.align_mask()
    }

    /// Returns the padded object size.
    ///
    /// This is also the allocation stride.
    #[inline(always)]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the padded object size.
    ///
    /// This is an alias for [`SizeClass::stride`] provided for code where
    /// the operation is conceptually about the object rather than the
    /// distance between objects.
    #[inline(always)]
    pub const fn padded_size(&self) -> usize {
        self.stride
    }

    /// Returns the number of bytes required to align `address`.
    ///
    /// The calculation is delegated to [`Layout::padding_for`] so that
    /// alignment arithmetic has exactly one implementation in the memory
    /// subsystem.
    #[inline(always)]
    pub const fn padding_for(&self, address: usize) -> usize {
        self.layout.padding_for(address)
    }

    /// Checks whether `address` satisfies this class's alignment.
    #[inline(always)]
    pub const fn is_aligned(&self, address: usize) -> bool {
        self.layout.is_aligned(address)
    }

    /// Aligns an address upward without checking for overflow.
    ///
    /// # Preconditions
    ///
    /// The caller must guarantee that the resulting aligned address is
    /// representable by `usize`.
    ///
    /// This method is intended for allocator hot paths after the relevant
    /// address range has already been validated.
    #[inline(always)]
    pub const fn align_up_unchecked(&self, address: usize) -> usize {
        self.layout.align_up_unchecked(address)
    }

    /// Returns the exclusive end address of an object.
    ///
    /// The calculation includes the complete padded object size.
    ///
    /// Returns [`LayoutError::AddressOverflow`] when the resulting end
    /// address cannot be represented by `usize`.
    #[inline(always)]
    pub const fn checked_end(
        &self,
        address: usize,
    ) -> Result<usize, LayoutError> {
        self.layout.checked_end(address)
    }

    /// Returns the exclusive end address without overflow checking.
    ///
    /// # Preconditions
    ///
    /// The caller must guarantee:
    ///
    /// `address + stride <= usize::MAX`
    ///
    /// This function is intended for validated allocator hot paths.
    #[inline(always)]
    pub const fn end_unchecked(&self, address: usize) -> usize {
        self.layout.end_unchecked(address)
    }

    /// Checks whether an object fits completely inside a memory region.
    ///
    /// `region_end_exclusive` is the exclusive end address of the region.
    ///
    /// The object is represented by:
    ///
    /// `[address, address + stride)`
    ///
    /// The operation returns `false` when the address arithmetic would
    /// overflow.
    #[inline(always)]
    pub const fn contains_range(
        &self,
        address: usize,
        region_end_exclusive: usize,
    ) -> bool {
        self.layout
            .contains_range(address, region_end_exclusive)
    }

    /// Returns whether this class can satisfy the requested layout.
    ///
    /// A class can satisfy a request when:
    ///
    /// `requested_size <= class_size`
    ///
    /// and
    ///
    /// `requested_align <= class_align`.
    ///
    /// Both alignments are guaranteed to be powers of two by [`Layout`].
    /// Therefore a class with an equal or greater alignment is also capable
    /// of providing the required alignment.
    ///
    /// This operation is constant-time and performs only integer
    /// comparisons.
    #[inline(always)]
    pub const fn fits_layout(&self, requested: Layout) -> bool {
        requested.size() <= self.size()
            && requested.align() <= self.align()
    }

    /// Returns whether this class can satisfy a raw size/alignment request.
    ///
    /// The request is validated through [`Layout::new`].
    ///
    /// This function is intended for control paths where the requested
    /// layout does not already exist.
    ///
    /// # Performance
    ///
    /// If the caller already has a validated [`Layout`], prefer
    /// [`SizeClass::fits_layout`] to avoid reconstructing it.
    #[inline]
    pub const fn fits(
        &self,
        size: usize,
        align: usize,
    ) -> bool {
        match Layout::new(size, align) {
            Ok(layout) => self.fits_layout(layout),
            Err(_) => false,
        }
    }

    /// Returns the number of complete objects of this class that fit in
    /// `bytes` bytes.
    #[inline(always)]
    pub const fn capacity_for(&self, bytes: usize) -> usize {
        bytes / self.stride
    }

    /// Returns the number of bytes occupied by `count` complete objects.
    ///
    /// Returns `None` if the multiplication would overflow `usize`.
    ///
    /// This operation is intended for validation and planning paths.
    #[inline(always)]
    pub const fn bytes_for(
        &self,
        count: usize,
    ) -> Option<usize> {
        self.stride.checked_mul(count)
    }

    /// Returns the largest number of complete objects that can fit in
    /// `bytes` bytes.
    ///
    /// This is equivalent to [`SizeClass::capacity_for`].
    #[inline(always)]
    pub const fn object_count_for(
        &self,
        bytes: usize,
    ) -> usize {
        self.capacity_for(bytes)
    }
}
