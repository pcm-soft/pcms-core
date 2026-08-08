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


use core::sync::atomic::{AtomicU64, Ordering};
use crate::{AllocError, AllocRS, Lease};

/// Verification wrapper. Counters are deliberately separate from the
/// production allocator and must not be used for WCET claims.
pub struct AllocRSChecked<const S: usize, const N: usize> {
    inner: AllocRS<S, N>, alloc_calls: AtomicU64, failures: AtomicU64,
}
impl<const S: usize, const N: usize> AllocRSChecked<S, N> {
    pub const fn new() -> Self {
        Self {
            inner: AllocRS::new(), alloc_calls: AtomicU64::new(0), failures: AtomicU64::new(0)
        }
    }

    pub fn allocate(&self) -> Result<Lease<'_, S, N>, AllocError> {
        self.alloc_calls.fetch_add(1, Ordering::Relaxed);
        match self.inner.allocate() {
            Ok(v) => Ok(v), Err(e) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }
    pub fn allocation_calls(&self) -> u64 {
        self.alloc_calls.load(Ordering::Relaxed)
    }
    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }
    pub fn inner(&self) -> &AllocRS<S, N> {
        &self.inner
    }
}
