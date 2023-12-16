# InterLog

**WORK IN PROGRESS**

## Overview

- Append events to log
- Sync log w/ other logs
- Event ID is composite of origin replica ID and log position
- Can run user-defined function to validate data before appending
- Can run user-defined function to validate data before syncing
- No interpreter or query language - recompile to change validation functions
- No data definition language. Events are just binary. User defined functions are binary -> error msg

## Implementation

- Direct I/O append only file, with 'working index' that maps ID's to log offsets
- Work at libc level (rustix), so you can follow man pages.
- Assume linux, 64 bit, little endian - for now
- Provide hooks to sync but actual HTTP (or whatever) server is out of scope
- Do the dumbest simplest thig you can and test the hell out of it
