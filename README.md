# InterLog

**WORK IN PROGRESS**

Interlog is a database optimized for writing and syncing data. It is both
embedded (designed to be written and read from in-process) and distributed
(full-support for synchronisation). In other words, it's Local-First: read and
write locally, sync remotely.

It's designed as an append only network of logs. Each log is a ledger of
events, which have a unique ID and an arbitrary byte array payload. Events are
recorded at each log in transaction order (ie, the order received by the log.)

## Design goals

- Deployable on cheap embedded linux devices. I want something commercially
  viable to slap onto shipping containers and trucks. (TODO: investigate if RTOS
  systems have the necessary primitives for allocation and file IO).
- Working offline: always available for writes no matter the network conditions. A log should never wait for the network for local IO.
- Fast; no mallocs after initialization, no disk IO save for appends. I plan to
  use Direct I/O to cache the "top" part of the log myself.
- Strong Eventual Consistency - Conflict free design based on CRDTs.
- Alternate implementations - it'd be interesting to see if I could run this in
  the browser, eg by compiling to WASM and writing to IndexedDB internally.

## Non-Goals

- A rich read model. Interlog is meant as a robust, but simple, source of truth.
  It allows you to append binary data, and read it back in transaction order.
  Any advanced read models should be derived in user code (Event Sourcing
  style).
- Linearizability or other strong consistency models. Interlog unashamedly in
  team Availability. ABW - always be writing.

## Planned Features (see [TODO file](TODO.md) for progress):

- Append events to log
- Sync log w/ other logs
- Event ID is composite of origin replica ID and log position
- Can run user-defined function to validate data before appending
- Can run user-defined function to validate data before syncing
- No interpreter or query language - recompile to change validation functions
- No data definition language. Events are just binary. User defined functions are binary -> error msg

## Implementation Docs

Documentation is in a state of flux, but start at core/src/actor.rs.
(TODO: document new modules, host on docs.rs)

## Sync Strategy

Logs are append only. So the causal state of each log can be expressed by a
single lamport clock - essentially the log length.

From there, we can capture the causal state of a distributed set of logs with a Version Vector per replica.

Looking at it from another angle, each replica constitutes a single GSet, and
sync is achieved by delta-state CDRT merge.
