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
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(rust_2018_idioms)]

mod checked;
mod error;
mod handle;
mod local;
mod shared;

pub use checked::AllocRSChecked;
pub use error::AllocError;
pub use handle::Handle;
pub use local::AllocRSLocal;
pub use shared::AllocRS;

/// Safe shared-arena ownership guard. Do not expose its Handle for safe
/// deallocation: that would allow freeing while a Lease still exists.
pub struct Lease<'a, const S: usize, const N: usize> {
    pub(crate) arena: &'a AllocRS<S, N>,
    pub(crate) handle: Handle
}

impl<'a, const S: usize, const N: usize> core::fmt::Debug for Lease<'a, S, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lease").finish()
    }
}

impl<'a, const S: usize, const N: usize> Lease<'a, S, N> {
    #[inline] pub fn as_slice(&self) -> &[u8] {
        let p = self.arena.ptr(self.handle).expect("Lease invariant");
        unsafe {
            core::slice::from_raw_parts(p as *const u8, S)
        }
    }
    #[inline] pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let p = self.arena.ptr(self.handle).expect("Lease invariant");
        unsafe {
            core::slice::from_raw_parts_mut(p, S)
        }
    }
}

impl<'a, const S: usize, const N: usize> Drop for Lease<'a, S, N> {
    fn drop(&mut self) {
        let _ = self.arena.release(self.handle);
    }
}

/// Safe local ownership guard.
pub struct LocalLease<'a, const S: usize, const N: usize> {
    pub(crate) arena: &'a mut AllocRSLocal<S, N>,
    pub(crate) handle: Handle
}

impl<'a, const S: usize, const N: usize> core::fmt::Debug for LocalLease<'a, S, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalLease").finish()
    }
}

impl<'a, const S: usize, const N: usize> LocalLease<'a, S, N> {
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let p = self.arena.ptr(self.handle).expect("LocalLease invariant");
        unsafe {
            core::slice::from_raw_parts_mut(p, S)
        }
    }
}
impl<'a, const S: usize, const N: usize> Drop for LocalLease<'a, S, N> {
    fn drop(&mut self) {
        let _ = self.arena.release(self.handle);
    }
}