extern crate alloc;
use alloc::boxed::Box;
use core::mem;

pub enum Err {
    Duplicate,
    LimitExceeded,
}

pub struct SlotMap<'a, T, const MAX_SLOTS: usize> {
    vals: &'a mut [T; MAX_SLOTS],
    /// Bitmask where 1 = occupied, 0 = available
    /// Allows us to remove values from the middle of the names array w/o
    /// re-ordering. If this array is empty, we've exceeded the capacity of
    /// values
    used_slots: u64,
}

impl<'a, T: Copy + Default + Eq, const MAX_SLOTS: usize>
    SlotMap<'a, T, MAX_SLOTS>
{
    const __: () = assert!((mem::size_of::<Slot>() * 8) > MAX_SLOTS);

    pub fn new(buf: &'a mut [T; MAX_SLOTS]) -> Self {
        Self { vals: buf, used_slots: 0 }
    }

    pub fn add(&mut self, val: T) -> Result<u8, Err> {
        if self.vals.contains(&val) {
            return Err(Err::Duplicate);
        }

        // Find first free slot
        let idx = (!self.used_slots).trailing_zeros() as u8;
        if idx >= MAX_SLOTS as u8 {
            return Err(Err::LimitExceeded);
        }

        // Mark slot as used
        self.used_slots |= 1u64 << idx;
        self.vals[idx as usize] = val;

        Ok(idx)
    }

    pub fn get(&self, slot: Slot) -> Option<T> {
        let nth_bit = (self.used_slots >> slot) & 1;
        (nth_bit == 1).then(|| self.vals[slot as usize])
    }

    pub fn remove(&mut self, slot: u8) -> T {
        assert!(slot < MAX_SLOTS as u8, "Index out of bounds");
        self.used_slots &= !(1u64 << slot);
        let name = &mut self.vals[slot as usize];
        mem::take(name)
    }
}

type Slot = u8;
