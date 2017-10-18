// This file is part of a6-tools.
// Copyright (C) 2017 Jeffrey Sharp
//
// a6-tools is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published
// by the Free Software Foundation, either version 3 of the License,
// or (at your option) any later version.
//
// a6-tools is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See
// the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with a6-tools.  If not, see <http://www.gnu.org/licenses/>.

// Distance to shift a BoolArray index to get the word index
#[cfg(target_pointer_width = "32")]
const WORD_INDEX_SHIFT: usize = 5;
#[cfg(target_pointer_width = "64")]
const WORD_INDEX_SHIFT: usize = 6;

// Value to mask a BoolArray index to get the bit-within-word index
const BIT_INDEX_MASK: usize = (1 << WORD_INDEX_SHIFT) - 1;

/// A fixed-length, packed array of `bool` values.
#[derive(Clone, Debug)]
pub struct BoolArray {
    words: Box<[usize]>,
    len:   usize,
}

impl BoolArray {
    /// Creates a `BoolArray` with the given length.
    pub fn new(len: usize) -> Self {
        // capacity is ceil(len / word_size)
        let cap = match len {
            0 => 0,
            n => 1 + (n - 1 >> WORD_INDEX_SHIFT),
        };
        Self {
            words: vec![0; cap].into_boxed_slice(),
            len
        }
    }

    /// Gets the length of the `BoolArray`.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Gets the `bool` value at the given `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    pub fn get(&self, index: usize) -> bool {
        assert!(index < self.len());
        let (index, mask) = split_index(index);
        let word = unsafe { self.words.get_unchecked(index) };
        word & mask != 0
    }

    /// Sets the `bool` value at the given `index` to `false` and returns the
    /// previous value.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    pub fn clear(&mut self, index: usize) -> bool {
        assert!(index < self.len());
        let (index, mask) = split_index(index);
        let slot = unsafe { self.words.get_unchecked_mut(index) };
        let word = *slot;
        *slot = word & !mask;
        word & mask != 0
    }

    /// Sets the `bool` value at the given `index` to `true` and returns the
    /// previous value.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    pub fn set(&mut self, index: usize) -> bool {
        assert!(index < self.len());
        let (index, mask) = split_index(index);
        let slot = unsafe { self.words.get_unchecked_mut(index) };
        let word = *slot;
        *slot = word | mask;
        word & mask != 0
    }

    /// Returns the index of the first `false` value, or `None` if all values
    /// in the `BitArray` are `true`.
    pub fn first_false(&self) -> Option<usize> {
        let     max   = usize::max_value();
        let mut index = 0;

        for &word in &*self.words {
            if word != max {
                index += (!word).trailing_zeros() as usize;
                if index < self.len() {
                    return Some(index)
                } else {
                    return None
                }
            }
            index += 1 << WORD_INDEX_SHIFT;
        }

        None
    }
}

#[inline]
fn split_index(index: usize) -> (usize, usize) {(
    // Index within words array
    index >> WORD_INDEX_SHIFT,

    // Mask for bit
    1 << (index & BIT_INDEX_MASK)
)}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use super::*;

    #[test]
    fn new() {
        let a = BoolArray::new(11);

        assert_eq!(a.len(), 11);
    }

    #[test]
    fn get() {
        let a = BoolArray::new(11);

        for i in 0..a.len() {
            assert_eq!(a.get(i), false);
        }
    }

    #[test]
    fn set() {
        let mut a = BoolArray::new(11);

        a.set(7);

        for i in 0..a.len() {
            assert_eq!(a.get(i), i == 7);
        }
    }

    #[test]
    fn clear() {
        let mut a = BoolArray::new(11);

        a.set(7);
        a.set(8);
        a.clear(7);

        for i in 0..a.len() {
            assert_eq!(a.get(i), i == 8);
        }
    }

    #[test]
    fn first_false_none() {
        let mut a = BoolArray::new(123);

        for i in 0..123 {
            a.set(i);
        }

        let i = a.first_false();

        assert_eq!(i, None);
    }

    #[test]
    fn first_false_some() {
        let mut a = BoolArray::new(123);

        for i in 0..123 {
            a.set(i);
        }

        a.clear(67);
        a.clear(99);

        let i = a.first_false();

        assert_eq!(i, Some(67));
    }
}

