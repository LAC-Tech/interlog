use std::slice::SliceIndex;

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
        let mut elems = Vec::with_capacity(capacity);
        unsafe { elems.set_len(capacity) };
        let elems = elems.into_boxed_slice();
        assert_eq!(std::mem::size_of_val(&elems), 16);
        Self {elems, len: 0}
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

    pub fn get<I>(&self, index: I) -> Option<&<I as SliceIndex<[T]>>::Output>
    where I: SliceIndex<[T]> {
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
        &self.elems[0..self.len]
    }
}

impl<T> std::ops::DerefMut for FixVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.elems[0..self.len]
    }
}

pub mod unit {
    use derive_more::*;

    /// Represents a byte address, divisible by 8, where an Event starts
    #[repr(transparent)]
    #[derive(Add, AddAssign, Clone, Copy, Debug, From, Into, PartialEq, Sub)]
    pub struct Byte(pub usize);

    impl Byte {
        pub fn align(self) -> Byte {
            Self((self.0 + 7)  & !7)
        }
    }

    #[repr(transparent)]
    #[derive(Add, AddAssign, Clone, Copy, From, Into)]
    #[derive(bytemuck::Pod, bytemuck::Zeroable, Debug)]
    pub struct Logical(pub usize);
}
