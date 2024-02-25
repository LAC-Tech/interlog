use std::slice::SliceIndex;

fn uninit_boxed_slice<T>(size: usize) -> Box<[T]> {
    let mut result = Vec::with_capacity(size);
    #[allow(clippy::uninit_vec)]
    unsafe { result.set_len(size) };
    result.into_boxed_slice()
}

pub trait Segmentable<T> {
    fn segment(&self, index: &Segment) -> Option<&[T]>;
}

/// Fixed Capacity Vector
/// Tigerstyle: There IS a limit
pub struct FixVec<T> {
    elems: alloc::boxed::Box<[T]>,
    len: usize
}

#[derive(Debug)]
pub struct FixVecOverflow;
pub type FixVecRes = Result<(), FixVecOverflow>;

impl<T> FixVec<T> {
    #[allow(clippy::uninit_vec)]
    pub fn new(capacity: usize) -> FixVec<T> {
        let elems = uninit_boxed_slice(capacity);
        assert_eq!(std::mem::size_of_val(&elems), 16);
        Self { elems, len: 0 }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.elems.len()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    fn check_capacity(&self, new_len: usize) -> FixVecRes {
        (self.capacity() >= new_len).then_some(()).ok_or(FixVecOverflow)
    }

    pub fn push(&mut self, value: T) -> FixVecRes {
        let new_len = self.len + 1;
        self.check_capacity(new_len)?;
        self.elems[self.len] = value;
        self.len = new_len;
        Ok(())
    }

    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) -> FixVecRes {
        for elem in iter {
            self.push(elem)?;
        }

        Ok(())
    }

    fn insert(&mut self, index: usize, element: T) -> FixVecRes {
        self.check_capacity(index + 1)?;
        self.elems[index] = element;
        Ok(())
    }

    fn get<I>(&self, index: I) -> Option<&<I as SliceIndex<[T]>>::Output>
    where
        I: SliceIndex<[T]>,
    {
        self.elems[..self.len].get(index)
    }
}

impl<T: Clone + core::fmt::Debug> FixVec<T> {
    pub fn resize(&mut self, new_len: usize, value: T) -> FixVecRes {
        self.check_capacity(new_len)?;

        if new_len > self.len {
            self.elems[self.len..new_len].fill(value);
        }

        self.len = new_len;

        Ok(())
    }
}

impl<T: Copy> FixVec<T> {
    pub fn extend_from_slice(&mut self, other: &[T]) -> FixVecRes {
        let new_len = self.len + other.len();
        self.check_capacity(new_len)?;
        self.elems[self.len..new_len].copy_from_slice(other);
        self.len = new_len;
        Ok(())
    }
}

impl<T> std::ops::Deref for FixVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elems[..self.len]
    }
}

impl<T> std::ops::DerefMut for FixVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.elems[..self.len]
    }
}

pub struct Segment {
    pub pos: usize,
    pub len: usize,
    pub end: usize
}

impl Segment {
    pub fn new(pos: usize, len: usize) -> Self {
        Self { pos, len, end: pos + len }
    }

    pub fn next(&self, len: usize) -> Self {
        Self::new(self.pos + self.len, len)
    }

    pub fn range(&self) -> core::ops::Range<usize> {
        self.pos..self.end
    }
}

impl<T> Segmentable<T> for FixVec<T> {
    fn segment(&self, index: &Segment) -> Option<&[T]> {
        self.elems[..self.len].get(index.range())
    }
}

pub struct CircBuf<T> {
    buffer: Box<[T]>,
    len: usize,
    write_idx: usize,
}

impl<T> CircBuf<T> {
    pub fn new(capacity: usize) -> Self {
        let buffer = uninit_boxed_slice(capacity);
        let len = 0;
        let write_idx = 0;
        Self { buffer, len, write_idx }
    }

    pub fn push(&mut self, item: T) {
        self.buffer[self.write_idx] = item;
        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        if self.len != self.buffer.len() {
            self.len += 1;
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if self.len == 0 { return None }
        let index = (index + self.write_idx).wrapping_rem_euclid(self.len);
        (self.len > index).then(|| &self.buffer[index])
    }
}

impl<T> CircBuf<T> {
    fn iter(&self) -> CircBufIterator<'_, T> {
        CircBufIterator {
            circ_buf: self,
            index: 0,
        }
    }
}

struct CircBufIterator<'a, T> {
    circ_buf: &'a CircBuf<T>,
    index: usize,
}

impl<'a, T> Iterator for CircBufIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        // This check prevents going around the circle infinitely
        if self.index < self.circ_buf.len {
            let item = self.circ_buf.get(self.index)?;
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }
}

pub mod unit {
    use core::fmt;
    use derive_more::*;

    /// Represents a byte address, divisible by 8, where an Event starts
    #[repr(transparent)]
    #[derive(Add, AddAssign, Clone, Copy, Debug, From, Into, PartialEq, Sub)]
    pub struct Byte(pub usize);

    impl Byte {
        pub fn align(self) -> Byte {
            Self((self.0 + 7) & !7)
        }
    }

    #[repr(transparent)]
    #[derive(Add, AddAssign, Clone, Copy, From, Into)] 
    #[derive(bytemuck::Pod, bytemuck::Zeroable)]
    pub struct Logical(pub usize);

    impl fmt::Display for Logical {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Logical({})", self.0)
        }
    }

    impl fmt::Debug for Logical {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn circ_buf() {
       let mut cb = CircBuf::new(4);
       
       // Preconditions
       assert_eq!(cb.iter().collect::<String>(), "");
       assert_eq!(cb.write_idx, 0);
       assert_eq!(cb.len, 0);

       cb.push('s');
       assert_eq!(cb.iter().copied().collect::<String>(), "s");
       assert_eq!(cb.write_idx, 1);
       assert_eq!(cb.len, 1);

       cb.push('i');
       assert_eq!(cb.iter().copied().collect::<String>(), "si");
       assert_eq!(cb.write_idx, 2);
       assert_eq!(cb.len, 2);

       cb.push('l');
       assert_eq!(cb.iter().collect::<String>(), "sil");
       assert_eq!(cb.write_idx, 3);
       assert_eq!(cb.len, 3);

       cb.push('m');
       assert_eq!(cb.iter().collect::<String>(), "silm");
       assert_eq!(cb.write_idx, 0);
       assert_eq!(cb.len, 4);

       cb.push('a');
       assert_eq!(cb.iter().collect::<String>(), "ilma");
       assert_eq!(cb.write_idx, 1);
       assert_eq!(cb.len, 4);

       cb.push('r');
       assert_eq!(cb.iter().collect::<String>(), "lmar");
       assert_eq!(cb.write_idx, 2);
       assert_eq!(cb.len, 4);
    }
}
