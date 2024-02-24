# Add new tasks to the top

- reconstruct LocalReplica from logfile
- implement 'read cache miss', so events on disk can be larger than read cache
- re-write event::read in terms of gettable + index
- impl index for circbuf, taking into account wrap around
- make a trait for "gettable" collections, make sure FixVec and CircBuf impl it
- make "event::read", "event::append_event" free-standing functions
~~- implement circular buffer for event read cache, with cache miss semantics ~~
~~- make all example based tests, property tests~~
~~- test utils file~~
~~- FCVec should have runtime capacity value~~
~~- create "FCVec<T>", replace FCBBuf with that~~
~~- Factor out EventBuf into its own struct~~
~~- write two values and read them both (using an index that stores the position)``
~~- read back using index~~
~~- write event header as well as event~~
~~- do the same but using fnctl with O_DIRECT~~
~~- write "hello, world!" to that file in a test, read it back, then delete it~~
~~- make replica struct~~
~~- delete file when done~~
~~- open and close path using direct I/O~~
~~- link to libC and import fcntl.h~~
~~- create testing file path "/tmp/interlog/randu80"~~
~~- print random u80 to console. then print it as hex~~
~~- comptime assert linux, 64 bit usize, little endian~~
