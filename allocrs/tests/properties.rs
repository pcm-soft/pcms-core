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
use proptest::prelude::*;

proptest! {
    #[test]
    fn arbitrary_sequences_do_not_lose_capacity(ops in prop::collection::vec(any::<bool>(), 1..500)) {
        let a = AllocRS::<64, 32>::new();
        let mut handles = Vec::new();
        for op in ops {
            if op {
                if let Ok(h) = unsafe {
                    a.allocate_raw()
                } { handles.push(h); }
            } else if let Some(h) = handles.pop() {
                prop_assert_eq!(unsafe { a.free_raw(h) },
                    Ok(())
                );
            }
            prop_assert!(a.allocated_slots() + a.free_slots() + a.retired_slots() == 32);
        }
        for h in handles { let _ = unsafe {
            a.free_raw(h)
        }; }
        prop_assert_eq!(a.allocated_slots(), 0);
        prop_assert_eq!(a.free_slots() + a.retired_slots(), 32);
    }
}

#[test]
fn explicit_errors_are_distinct() {
    let a = AllocRS::<64, 1>::new();
    let h = unsafe {
        a.allocate_raw().unwrap()
    };
    assert_eq!(
        unsafe {
        a.free_raw(h) },
               Ok(())
    );
    assert_eq!(
        unsafe {
        a.free_raw(h)
    }, Err(AllocError::DoubleFree));
}
