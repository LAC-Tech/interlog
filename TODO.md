~~- comptime assert linux, 64 bit usize, little endian~~
~~- print random u80 to console. then print it as hex~~
~~- create testing file path "/tmp/interlog/randu80"~~
~~- link to libC and import fcntl.h~~
~~- open and close path using direct I/O~~
~~- delete file when done~~
~~- make replica struct~~
- write "hello, world!" to that file in a test, read it back, then
  delete it
- do the same but using fnctl with O_DIRECT
- test appending numbers to the file. With and without a close in-between.
- replica struct. 80 bit RNG address, u64 ArrayList. re-write prev tests
- EventID, 80 bit origin address, 48 bit log_pos. re-write previous test with
  id/number pairs
