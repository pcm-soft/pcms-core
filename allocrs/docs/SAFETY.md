# Safety Case Skeleton

## Safety objectives

- No dynamic allocation after initialization.
- No use-after-free through the safe API.
- No stale-handle release through the safe API.
- No generation wraparound: exhausted slots retire.
- No out-of-range slot indexing from public safe operations.
- Shared mode contains no blocking mutex.
- Local mode contains no atomic operation in allocation/free hot paths.

## Unsafe code inventory

There are three categories of unsafe code:

1. `UnsafeCell` for storage mutation through a shared arena reference.
2. `from_raw_parts` / `from_raw_parts_mut` in `Lease`.
3. `unsafe allocate_raw/free_raw` integration APIs.

Each unsafe block has a local invariant. Any change to these invariants requires re-review and re-running the full verification suite.

## Known limitations

- Shared mode is lock-free, not a hard WCET proof.
- Generation space is finite; exhaustion retires a slot.
- Memory contents are not scrubbed on free.
- `S` is constrained to a multiple of 64; this is a layout/performance policy, not a medical safety guarantee.
- A system integrator must define interrupt, DMA, cache-coherency and MPU/MMU ownership rules.
