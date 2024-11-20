# Top = TODO first

- version vector inside event buf, to keep it consistent
- read from end should not fill an event buf. It should return an iterator. but
  it needs a chunk of bytes to fill up. maybe block sized? multiple of block
  sized? 
- Document every module
- upload to docs.rs
