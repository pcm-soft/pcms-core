# Threat Model

The allocator assumes trusted in-process code and does not defend against a malicious caller that deliberately violates an `unsafe` contract.

## Threats addressed by the safe API

- stale ownership tokens;
- double release;
- invalid indices;
- generation reuse;
- accidental concurrent ownership of one slot.

## Threats not addressed

- arbitrary memory corruption outside the arena;
- DMA writing into arena memory without an ownership protocol;
- hardware faults;
- cache coherency failures;
- malicious unsafe Rust;
- incorrect MPU/MMU configuration;
- incorrect system-level synchronization.
