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

use core::fmt;

/// Opaque allocation token. It is intentionally not a capability for safe
/// mutable access;
/// safe access is provided only by Lease.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Handle(u64);
impl Handle {
    #[inline(always)]
    pub(crate) const fn new(index: usize, generation: u32) -> Self {
        Self(((generation as u64) << 32) | index as u64)
    }
    #[inline(always)]
    pub(crate) const fn index(self) -> usize {
        self.0 as u32 as usize
    }
    #[inline(always)]
    pub(crate) const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}
impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}
