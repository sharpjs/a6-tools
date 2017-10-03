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

use std::cmp::min;

// The alignment in bytes for `usize` values.
#[cfg(target_pointer_width = "32")]
const USIZE_BYTES: usize = 4;
#[cfg(target_pointer_width = "64")]
const USIZE_BYTES: usize = 8;

// A mask of the bits that must be zero in a `usize`-aligned address.
const UNALIGNED_MASK: usize = USIZE_BYTES - 1;

/// Extension methods for pointer arithmetic and alignment.
///
/// Some of these methods are being implemented in the Rust standard library,
/// but they are not yet available in stable Rust.
///
pub trait PointerExt: Copy {
    /// Adds an offset of `count * size_of::<T>()` to the pointer.
    ///
    /// A standard library implementation is in progress:
    /// https://github.com/rust-lang/rfcs/blob/master/text/1966-unsafe-pointer-reform.md
    unsafe fn add(self, count: usize) -> Self;

    /// Subtracts an offset of `count * size_of::<T>()` from the pointer.
    ///
    /// A standard library implementation is in progress:
    /// https://github.com/rust-lang/rfcs/blob/master/text/1966-unsafe-pointer-reform.md
    unsafe fn sub(self, count: usize) -> Self;

    /// Calculates the forward offset in bytes from the pointer to the given
    /// `other` pointer.
    fn byte_len_to<U>(self, other: *const U) -> usize;

    /// Calculates the forward offset in bytes from the pointer to the nearest
    /// `usize`-aligned address.  Returns 0 if the pointer is aligned already.
    ///
    /// A standard library implementation is in progress:
    /// https://github.com/rust-lang/rfcs/blob/master/text/2043-is-aligned-intrinsic.md
    fn offset_to_aligned(self) -> usize;

    /// Calculates the forward offset in bytes from the nearest preceding
    /// `usize`-aligned address to the pointer.  Returns 0 if the pointer is
    /// aligned already.
    fn offset_from_aligned(self) -> usize;
    
    /// Adds to the pointer the minimum offset sufficient to align the pointer
    /// to a `usize` boundary.
    unsafe fn align_up(self) -> Self {
        self.add(self.offset_to_aligned())
    }

    /// Subtracts from the pointer the minimum offset sufficient to align the
    /// pointer to a `usize` boundary.
    unsafe fn align_down(self) -> Self {
        self.sub(self.offset_from_aligned())
    }
}

impl<T> PointerExt for *const T {
     #[inline(always)]
     unsafe fn add(self, count: usize) -> Self {
         self.offset(count as isize)
     }

     #[inline(always)]
     unsafe fn sub(self, count: usize) -> Self {
         self.offset((count as isize).wrapping_neg())
     }

     #[inline]
     fn byte_len_to<U>(self, other: *const U) -> usize {
         (other as usize).wrapping_sub(self as usize)
     }

     #[inline]
     fn offset_to_aligned(self) -> usize {
         !(self as usize + UNALIGNED_MASK) & UNALIGNED_MASK
     }

     #[inline]
     fn offset_from_aligned(self) -> usize {
         self as usize & UNALIGNED_MASK
     }
}

/// A trait that enables searching a collection for items having specific bits
/// set or unset.
pub trait FindBits {
    type Index: Copy;
    type Item:  Copy;

    fn find_bits(&self, bits: Self::Item, mask: Self::Item)
        -> Option<(Self::Index, Self::Item)>;
}

impl FindBits for [u8]  {
    type Index = usize;
    type Item  = u8;

    fn find_bits(&self, bits: u8, mask: u8) -> Option<(usize, u8)> {
        unsafe {
            // Compute bounds
            let mut ptr = self.as_ptr();
            let     beg = ptr;
            let     end = ptr.add(self.len());

            // Zero the bits caller doesn't care about
            let bits = bits & mask;

            // Check byte-wise up to usize-aligned location
            let aligned = min(ptr.align_up(), end);
            while ptr < aligned {
                let value = *ptr;
                if value & mask == bits {
                    return Some((beg.byte_len_to(ptr), value));
                }
                ptr = ptr.add(1);
            }

            // Check usize-wise up to last usize-aligned location
            let bits_wide = fill_usize(bits);
            let mask_wide = fill_usize(mask);
            let aligned   = end.align_down();
            while ptr < aligned {
                let value = *(ptr as *const usize) & mask_wide ^ bits_wide;
                if has_zero_byte(value) { break }
                ptr = ptr.add(USIZE_BYTES);
            }

            // Check remaining bytes
            while ptr < end {
                let value = *ptr;
                if value & mask == bits {
                    return Some((beg.byte_len_to(ptr), value));
                }
                ptr = ptr.add(1);
            }
        }

        return None
    }
}

#[cfg(target_pointer_width = "32")]
#[inline]
fn fill_usize(b: u8) -> usize {
    let mut x = b as usize; // 0x_0000_00nn
    x |= x <<  8;           // 0x_0000_nnnn
    x |= x << 16;           // 0x_nnnn_nnnn
    x
}

#[cfg(target_pointer_width = "64")]
#[inline]
fn fill_usize(b: u8) -> usize {
    let mut x = b as usize; // 0x_0000_0000_0000_00nn
    x |= x <<  8;           // 0x_0000_0000_0000_nnnn
    x |= x << 16;           // 0x_0000_0000_nnnn_nnnn
    x |= x << 32;           // 0x_nnnn_nnnn_nnnn_nnnn
    x
}

#[inline]
fn has_zero_byte(x: usize) -> bool {
    // Method from 1987-04-27 USENET post by Alan Mycroft
    const ALL_0x01: usize = 0x0101010101010101u64 as usize;
    const ALL_0x80: usize = 0x8080808080808080u64 as usize;
    x.wrapping_sub(ALL_0x01) & !x & ALL_0x80 != 0
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use super::*;

    static INTS: [i32; 3] = [11, 22, 33];

    static BYTES: [u8; 16] = [
        0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
        0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF,
    ];

    #[test]
    fn add() {
        let ptr = INTS[1..].as_ptr();

        let item = unsafe { *(ptr.add(1)) };

        assert_eq!(item, 33);
    }

    #[test]
    fn sub() {
        let ptr = INTS[1..].as_ptr();

        let item = unsafe { *(ptr.sub(1)) };

        assert_eq!(item, 11);
    }

    #[test]
    fn byte_len_to() {
        let ptr1 = INTS[1..].as_ptr();
        let ptr2 = INTS[2..].as_ptr();

        let len11 = ptr1.byte_len_to(ptr1);
        let len12 = ptr1.byte_len_to(ptr2);
        let len21 = ptr2.byte_len_to(ptr1);

        assert_eq!(len11, 0usize);
        assert_eq!(len12, 4usize);
        assert_eq!(len21, 4usize.wrapping_neg());
    }

    #[test]
    fn offset_to_aligned() {
        let align = size_of::<usize>();

        for i in 0..(2 * align + 1) {
            let ptr      = i as *const u8;
            let actual   = ptr.offset_to_aligned();
            let expected = match i % align {
                0 => 0,
                i => align - i,
            };

            assert_eq!(actual, expected, "at offset {}", i);
        }
    }

    #[test]
    fn offset_from_aligned() {
        let align = size_of::<usize>();

        for i in 0..(2 * align + 1) {
            let ptr      = i as *const u8;
            let actual   = ptr.offset_from_aligned();
            let expected = i % align;

            assert_eq!(actual, expected, "at offset {}", i);
        }
    }

    #[test]
    fn align_up() {
        let unaligned = 1 as *const u8;
        let   aligned = unsafe { unaligned.align_up() };

        assert!(unaligned < aligned);
    }

    #[test]
    fn align_down() {
        let unaligned = 1 as *const u8;
        let   aligned = unsafe { unaligned.align_down() };

        assert!(aligned < unaligned);
    }

    #[test]
    fn find_bits_found() {
        const MASK: u8 = 0b_0000_1111;
        let align = size_of::<usize>();

        // Test every initial (mis)alignment
        for offset in 0..align {
            let bytes = &BYTES[offset..];

            // Test every found position
            for (i, &byte) in bytes.iter().enumerate() {
                let found = bytes.find_bits(byte, MASK);
                assert_eq!(found, Some((i, byte)));
            }
        }
    }

    #[test]
    fn find_bits_not_found() {
        const BYTE: u8 = 0b_0101_0011;
        const MASK: u8 = 0b_0111_0000;
        let align = size_of::<usize>();

        // Test every initial (mis)alignment
        for offset in 0..align {
            let bytes = &BYTES[offset..];

            let result = bytes.find_bits(BYTE, MASK);

            assert_eq!(result, None);
        }
    }
}

