extern crate alloc;
use alloc::boxed::Box;
use core::mem;

const MAX: u8 = 64;

pub enum Err {
    Duplicate,
    LimitExceeded,
}

pub struct SlotMap<T> {
    /// Using an array so we can give each name a small "address"
    /// Ensure size is less than 256 bytes so we can use a u8 as the index
    vals: Box<[T; MAX as usize]>,
    /// Bitmask where 1 = occupied, 0 = available
    /// Allows us to remove names from the middle of the names array w/o
    /// re-ordering. If this array is empty, we've exceeded the capacity of
    /// names
    used_slots: u64,
}

impl<T: Copy + Default + Eq> SlotMap<T> {
    pub fn new() -> Self {
        Self { vals: Box::new([T::default(); MAX as usize]), used_slots: 0 }
    }

    pub fn add(&mut self, val: T) -> Result<u8, Err> {
        if self.vals.contains(&val) {
            return Err(Err::Duplicate);
        }

        // Find first free slot
        let idx = (!self.used_slots).trailing_zeros() as u8;
        if idx >= MAX {
            return Err(Err::LimitExceeded);
        }

        // Mark slot as used
        self.used_slots |= 1u64 << idx;
        self.vals[idx as usize] = val;

        Ok(idx)
    }

    pub fn remove(&mut self, slot: u8) -> T {
        assert!(slot < MAX, "Index out of bounds");
        self.used_slots &= !(1u64 << slot);
        let name = &mut self.vals[slot as usize];
        mem::take(name)
    }
}
