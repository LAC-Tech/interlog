# InterLog

**WORK IN PROGRESS**

## Overview

A distributed, local first log.

Planned Featurs (see [TODO file](TODO.md) for progress):

- Append events to log
- Sync log w/ other logs
- Event ID is composite of origin replica ID and log position
- Can run user-defined function to validate data before appending
- Can run user-defined function to validate data before syncing
- No interpreter or query language - recompile to change validation functions
- No data definition language. Events are just binary. User defined functions are binary -> error msg

## Implementation Docs

Each source file is documented, start at lib.rs, and then probably replica.rs

## Sync Strategy

(TODO: I prototyped this in another repo, figure out which one and link it).

Logs are append only. So the causal state of each log can be expressed by a
single lamport clock - essentially the log length.

From there, we can capture the causal state of a distributed set of logs with a Version Vector per replica.

Looking at it from another angle, each replica constitutes a single GSet, and
sync is achieved by delta-state CDRT merge.
