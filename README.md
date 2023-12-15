# InterLog

**WORK IN PROGRESS**

## Overview

- Append events to log
- Sync log w/ other logs
- Bi-temporal: records (local) transaction time & valid time
- Event ID is a ULID where the time part is the valid time
- Can run user-defined function to validate data before appending
- Can run user-defined function to validate data before syncing
- No interpreter or query language - recompile to change validation functions
- No data definition language. Events are just binary. User defined functions are binary -> error msg

## Implementation

- Direct I/O append only file, with 'working index' that maps ID's to log offsets
- Linux only (for now)
- Call functions that have man pages rather than relying on
  wrappers or std libraries
- Do the dumbest simplest thig you can and test the hell out of it
