# PCMS

**PCMS** is a `no_std`, zero-allocation, lock-free foundation for trusted medical / imaging plugins running in the same privilege domain as the kernel (UEFI / bare-metal / EL1 / S-mode).

It is **not** a security isolation boundary. Plugins are trusted kernel components.

## Components

| Crate       | Role |
|-------------|------|
| `pcms-core` | UEFI entry, MPSC bus, plugin executor, timer |
| `pcms-sdk`  | Versioned C ABI for trusted plugins (MedicalPacket, host API, capabilities) |
| `allocrs`   | Fixed-capacity lock-free / single-owner arena allocators (shared + local) |

## Design goals

- **Realtime** — producers never block; full queue returns immediately (`BusError::Full`).
- **Zero allocation** on the hot path — no heap, no `alloc` crate required.
- **Lock-free MPSC** bus based on per-slot sequence numbers.
- **Bounded** everything — fixed capacity, compile-time sizes, power-of-two queues.
- **Trusted-plugin ABI** — same address space, minimal transition cost, explicit capabilities.
- **`no_std` + UEFI** first-class.

## Quick start (QEMU)

```bash
./build_qemu.py