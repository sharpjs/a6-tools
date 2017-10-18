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
}

impl BoolArray {
    /// Creates a `BoolArray` with at least the given length.
    /// The actual length will be `len` rounded up to the machine word size.
    pub fn new(len: usize) -> Self {
        let len = match split_index(len) {
            (len, 1) => len,
            (len, _) => len + 1,
        };
        Self { words: vec![0; len].into_boxed_slice() }
    }

    /// Gets the length of the `BoolArray`.
    pub fn len(&self) -> usize {
        self.words.len() << WORD_INDEX_SHIFT
    }

    /// Gets the `bool` value at the given `index`.
    pub fn get(&self, index: usize) -> bool {
        let (index, mask) = split_index(index);
        self.words[index] & mask != 0
    }

    /// Sets the `bool` value at the given `index` to `false`.
    /// Returns the previous value.
    pub fn clear(&mut self, index: usize) -> bool {
        let (index, mask) = split_index(index);
        let word = self.words[index];
        self.words[index] = word & !mask;
        word & mask != 0
    }

    /// Sets the `bool` value at the given `index` to `true`.
    /// Returns the previous value.
    pub fn set(&mut self, index: usize) -> bool {
        let (index, mask) = split_index(index);
        let word = self.words[index];
        self.words[index] = word | mask;
        word & mask != 0
    }
}

#[inline]
fn split_index(index: usize) -> (usize, usize) {
    (
        // Index within words array
        index >> WORD_INDEX_SHIFT,
        // Mask for bit
        1 << (index & BIT_INDEX_MASK)
    )
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use super::*;

    #[test]
    fn new_nonword_size() {
        let bitness = size_of::<usize>() * 8;

        let a = BoolArray::new(11);

        assert_eq!(a.len(), bitness);
    }

    #[test]
    fn new_word_size() {
        let bitness = size_of::<usize>() * 8;

        let a = BoolArray::new(bitness);

        assert_eq!(a.len(), bitness);
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
}

