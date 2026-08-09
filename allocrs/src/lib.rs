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

#![no_std]
#![feature(generic_const_exprs)]
#![allow(incomplete_features)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(rust_2018_idioms)]

mod checked;
mod error;
mod handle;
mod local;
mod shared;
mod utils;

pub use checked::AllocRSChecked;
pub use error::AllocError;
pub use handle::Handle;
pub use local::AllocRSLocal;
pub use shared::AllocRS;
pub use utils::words;

/// Safe shared-arena ownership guard.
///
/// The handle is intentionally kept private so safe code cannot release
/// the allocation while the lease is alive.
pub struct Lease<'a, const S: usize, const N: usize> {
    pub(crate) arena: &'a AllocRS<S, N>,
    pub(crate) handle: Handle,
}

impl<'a, const S: usize, const N: usize> core::fmt::Debug
    for Lease<'a, S, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lease").finish()
    }
}

impl<'a, const S: usize, const N: usize> Lease<'a, S, N> {
    /// Returns the entire slot as an immutable byte slice.
    ///
    /// The lease guarantees that the allocation remains owned for the
    /// lifetime of the returned reference.
    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        let p = self
            .arena
            .ptr(self.handle)
            .expect("Lease invariant violated");

        // SAFETY:
        //
        // `ptr()` validates the handle and therefore guarantees that:
        //
        // * the slot index is within the arena;
        // * the slot is allocated;
        // * the generation matches this lease.
        //
        // The slot contains exactly S bytes.
        unsafe { core::slice::from_raw_parts(p as *const u8, S) }
    }

    /// Returns the entire slot as a mutable byte slice.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let p = self
            .arena
            .ptr(self.handle)
            .expect("Lease invariant violated");

        // SAFETY:
        //
        // The lease is the unique safe owner of this allocation.
        // The arena does not expose another safe mutable reference to the
        // same slot while this lease exists.
        unsafe { core::slice::from_raw_parts_mut(p, S) }
    }
}

impl<'a, const S: usize, const N: usize> Drop for Lease<'a, S, N> {
    fn drop(&mut self) {
        let _ = self.arena.release(self.handle);
    }
}

/// Safe single-owner arena lease.
pub struct LocalLease<'a, const S: usize, const N: usize> {
    pub(crate) arena: &'a mut AllocRSLocal<S, N>,
    pub(crate) handle: Handle,
}

impl<'a, const S: usize, const N: usize> core::fmt::Debug
    for LocalLease<'a, S, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalLease").finish()
    }
}

impl<'a, const S: usize, const N: usize> LocalLease<'a, S, N> {
    /// Returns mutable access to the entire slot.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let p = self
            .arena
            .ptr(self.handle)
            .expect("LocalLease invariant violated");

        // SAFETY:
        //
        // The lifetime of the returned slice is bounded by `&mut self`.
        // `LocalLease` owns exclusive mutable access to the local arena.
        unsafe { core::slice::from_raw_parts_mut(p, S) }
    }
}

impl<'a, const S: usize, const N: usize> Drop for LocalLease<'a, S, N> {
    fn drop(&mut self) {
        let _ = self.arena.release(self.handle);
    }
}