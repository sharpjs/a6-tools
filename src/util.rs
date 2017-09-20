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

/// Extension methods for pointer arithmetic and alignment.
///
/// Some of these methods are being implemented in the standard library, but
/// they are not yet available in Rust.
///
pub trait PointerExt: Copy {
    /// Adds an offset of `count * size_of::<T>()` to the pointer.
    ///
    /// A standard library implementation is in progress:
    /// https://github.com/rust-lang/rfcs/blob/master/text/1966-unsafe-pointer-reform.md
    unsafe fn add(self, count: usize) -> Self;

    /// Subtracts an offset of `count * size_of::<T>()` to the pointer.
    ///
    /// A standard library implementation is in progress:
    /// https://github.com/rust-lang/rfcs/blob/master/text/1966-unsafe-pointer-reform.md
    unsafe fn sub(self, count: usize) -> Self;

    /// Calculates the offset in bytes from the pointer to the given `other`
    /// pointer.
    fn byte_len_to<U>(self, other: *const U) -> usize;
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

     #[inline(always)]
     fn byte_len_to<U>(self, other: *const U) -> usize {
         (other as usize).wrapping_sub(self as usize)
     }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ITEMS: [i32; 3] = [11, 22, 33];

    #[test]
    fn add() {
        let ptr = ITEMS[1..].as_ptr();

        let item = unsafe { *(ptr.add(1)) };

        assert_eq!(item, 33);
    }

    #[test]
    fn sub() {
        let ptr = ITEMS[1..].as_ptr();

        let item = unsafe { *(ptr.sub(1)) };

        assert_eq!(item, 11);
    }

    #[test]
    fn byte_len_to() {
        let ptr1 = ITEMS[1..].as_ptr();
        let ptr2 = ITEMS[2..].as_ptr();

        let len11 = ptr1.byte_len_to(ptr1);
        let len12 = ptr1.byte_len_to(ptr2);
        let len21 = ptr2.byte_len_to(ptr1);

        assert_eq!(len11, 0usize);
        assert_eq!(len12, 4usize);
        assert_eq!(len21, 4usize.wrapping_neg());
    }
}

