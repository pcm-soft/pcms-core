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

use allocrs::AllocRS;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn concurrent_allocate_release_preserves_capacity() {
    const N: usize = 128;
    let arena = Arc::new(AllocRS::<64, N>::new());
    let barrier = Arc::new(Barrier::new(8));
    let mut threads = Vec::new();

    for _ in 0..8 {
        let arena = Arc::clone(&arena);
        let barrier = Arc::clone(&barrier);
        threads.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..10_000 {
                if let Ok(mut lease) = arena.allocate() {
                    lease.as_mut_slice()[0] = 0xA5;
                }
            }
        }));
    }

    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(arena.allocated_slots(), 0);
    assert_eq!(arena.free_slots(), N);
    assert_eq!(arena.retired_slots(), 0);
}
