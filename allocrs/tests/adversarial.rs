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

use allocrs::{AllocError, AllocRS};

#[test]
fn stale_handle_never_releases_new_owner() {
    let a = AllocRS::<64, 1>::new();
    let old = unsafe {
        a.allocate_raw().unwrap()
    };
    unsafe {
        a.free_raw(old).unwrap()
    };
    let new = unsafe {
        a.allocate_raw().unwrap()
    };
    assert_eq!(unsafe {
        a.free_raw(old)
    }, Err(AllocError::StaleHandle));
    assert_eq!(a.allocated_slots(), 1);
    unsafe {
        a.free_raw(new).unwrap()
    };
}

#[test]
fn invalid_handle_is_rejected() {
    let a = AllocRS::<64, 2>::new();
    let forged = unsafe {
        core::mem::transmute::<u64,
            allocrs::Handle>(u64::MAX)
    };
    assert_eq!(
        unsafe {
        a.free_raw(forged)
    }, Err(AllocError::InvalidHandle));
}

#[test]
fn capacity_accounting_is_conservative() {
    let a = AllocRS::<64, 65>::new();
    let mut hs = [None; 65];
    for h in &mut hs {
        *h = Some(
            unsafe {
            a.allocate_raw().unwrap()
        });
    }
    assert_eq!(a.free_slots(), 0);
    for h in hs {
        unsafe {
        a.free_raw(h.unwrap()).unwrap();
        }
    }
    assert_eq!(a.free_slots(), 65);
}
