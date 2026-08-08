# Architecture

```text
                         +-----------------------+
                         |      Application      |
                         +-----------+-----------+
                                     |
                 +-------------------+-------------------+
                 |                                       |
          safe Lease API                         unsafe integration
                 |                                       |
        +--------+--------+                         Handle API
        |                 |
   AllocRSLocal       AllocRS
   single owner       shared lock-free
        |                 |
        |                 +-- SlotState atomics
        |                 +-- free bitmap
        |                 +-- summary bitmap
        |
        +-- plain bitmap / counters

Verification:
unit -> property -> concurrency -> fuzz -> Miri -> Loom -> target hardware
```

The safe API is intentionally capability-oriented: storage mutation is tied
to a live Lease. Raw handles exist only at explicit unsafe integration
boundaries.
