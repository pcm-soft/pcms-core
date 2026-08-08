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


use allocrs::{AllocRS, AllocRSLocal};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_shared(c: &mut Criterion) {
    c.bench_function("shared_allocate_free", |b| {
        let a = AllocRS::<256, 256>::new();
        b.iter(|| {
            let lease = black_box(a.allocate().unwrap());
            black_box(lease);
        });
    });
}

fn bench_local(c: &mut Criterion) {
    c.bench_function("local_allocate_free", |b| {
        let mut a = AllocRSLocal::<256, 256>::new();
        b.iter(|| {
            let lease = black_box(a.allocate().unwrap());
            black_box(lease);
        });
    });
}

criterion_group!(benches, bench_shared, bench_local);
criterion_main!(benches);
