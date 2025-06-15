//! Heap and Stack allocated vectors that
//! - have a fixed capacity
//! - don't reallocate

use core::convert::AsRef;
use core::ops::IndexMut;

pub type Stack<T, const CAPACITY: usize> = Vec<[T; CAPACITY], CAPACITY>;
pub type Heap<'a, T, const CAPACITY: usize> = Vec<&'a mut [T], CAPACITY>;

pub fn create_on_stack<T, const CAPACITY: usize>() -> Stack<T, CAPACITY>
where
    T: Copy + Default,
{
    Vec { mem: [T::default(); CAPACITY], len: 0 }
}

pub fn create_from_buf<'a, T, const CAPACITY: usize>(
    mem: &'a mut [T],
) -> Heap<'a, T, CAPACITY> {
    Vec { mem, len: 0 }
}

pub struct Vec<M, const CAPACITY: usize> {
    mem: M,
    len: usize,
}

impl<T, M, const CAPACITY: usize> Vec<M, CAPACITY>
where
    T: Copy + Default,
    M: AsRef<[T]> + IndexMut<usize, Output = T>,
{
    pub fn push(&mut self, value: T) -> Result<(), Err> {
        (self.len >= CAPACITY).then_some(()).ok_or(Err::Overflow)?;
        self.mem[self.len] = value;
        self.len += 1;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn as_slice(&self) -> &[T] {
        let slice: &[T] = self.mem.as_ref();
        &slice[0..self.len]
    }
}

pub enum Err {
    Overflow,
}
