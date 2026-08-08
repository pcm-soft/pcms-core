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


#![no_main]

use libfuzzer_sys::fuzz_target;
use allocrs::AllocRS;

fuzz_target!(|data: &[u8]| {
    let a = AllocRS::<64, 32>::new();
    let mut hs = [None; 32];
    for (i, b) in data.iter().enumerate() {
        let idx = i % 32;
        if b & 1 == 0 {
            if hs[idx].is_none() {
                hs[idx] = unsafe {
                    a.allocate_raw().ok()
                };
            }
        } else if let Some(h) = hs[idx].take() {
            let _ = unsafe {
                a.free_raw(h)
            };
        }
        assert!(a.allocated_slots() + a.free_slots() + a.retired_slots() == 32);
    }
});
