~~- comptime assert linux, 64 bit usize, little endian~~
~~- print random u80 to console. then print it as hex~~
~~- create testing file path "/tmp/interlog/randu80"~~
~~- link to libC and import fcntl.h~~
~~- open and close path using direct I/O~~
~~- delete file when done~~
~~- make replica struct~~
~~- write "hello, world!" to that file in a test, read it back, then delete it~~
~~- do the same but using fnctl with O_DIRECT~~
~~- read and write through virtual 4KiB sectors~~
- write two values and read them both (using an index that stores the position)
- reconstruct index from log (will need event metadata on log)
