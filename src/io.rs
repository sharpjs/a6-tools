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

use std::io::prelude::*;
use std::io::{self, Error};
use std::io::ErrorKind::{Interrupted, UnexpectedEof};
use util::FindBits;

/// Extension methods for `std::io::Error`.
pub trait ErrorExt {
    /// Returns `true` if the error is a transient error, `false` otherwise.
    fn is_transient(&self) -> bool;
}

impl ErrorExt for Error {
    #[inline]
    fn is_transient(&self) -> bool {
        self.kind() == Interrupted
    }
}

macro_rules! def_read {
    {
        $( $name:ident ( $n:expr, $v:ident: $t:ty ) { $e:expr } )*
    } => {
        $(
            /// Reads a `$t`.
            ///
            /// # Errors
            ///
            /// Error behavior is identical to `std::io::Read::read_exact`:
            ///
            /// * `ErrorKind::Interrupted` errors are ignored.
            ///
            /// * Other errors indicate failure.  Actual number of bytes read is
            ///   unspecified, other than <= size of `$t`.
            ///
            fn $name(&mut self) -> io::Result<$t> {
                use std::mem;

                // Read into temporary buffer
                let mut buf = [0; $n];
                self.read_exact(&mut buf)?;

                // Interpret as desired type
                let $v: $t = unsafe { mem::transmute(buf) };
                Ok($e)
            }
        )*
    }
}

pub trait ReadExt: Read {
    def_read! {
        read_u8  (1, v: u8 ) { v         }
        read_u16 (2, v: u16) { v.to_be() }
        read_u32 (4, v: u32) { v.to_be() }
    }
}

impl<R: Read> ReadExt for R { }

pub trait BufReadExt {
    /// Consumes bytes until one matches the given bit pattern or EOF is reached.
    /// To match, a byte must equal `bits` in the bit positions corresponding to
    /// the 1-bits in `mask`.
    ///
    /// Non-matching bytes are passed in nonzero-length slices to the given
    /// handler `f` for arbitrary processing.
    ///
    /// Returns a tuple `(count, found)` indicating how many non-matching bytes
    /// were consumed and the matching byte, if any.
    ///
    /// On return, if a byte matched, the stream is positioned at the following
    /// byte. Otherwise, the stream is positioned at EOF.
    fn scan_until_bits<F>(&mut self, bits: u8, mask: u8, f: F)
        -> io::Result<(usize, Option<u8>)>
    where
        F: FnMut(&[u8]);

    /// Reads and discards bytes until one matches the given bit pattern or EOF
    /// is reached.  To match, a byte must equal `bits` in the bit positions
    /// corresponding to the 1-bits in `mask`.
    ///
    /// Returns a tuple `(count, found)` indicating how many non-matching bytes
    /// were discarded and the matching byte, if any.
    ///
    /// On return, if a byte matched, the stream is positioned at the following
    /// byte. Otherwise, the stream is positioned at EOF.
    fn skip_until_bits(&mut self, bits: u8, mask: u8)
        -> io::Result<(usize, Option<u8>)>
    {
        self.scan_until_bits(bits, mask, |_| {})
    }

    /// Reads bytes into `buf` until one matches the given bit pattern or EOF
    /// is reached.  To match, a byte must equal `bits` in the bit positions
    /// corresponding to the 1-bits in `mask`.
    ///
    /// Non-matching bytes are copied to `buf` as its length permits.  If the
    /// read exceeds the length of `buf`, additional non-matching bytes are
    /// discarded.  If a byte matches, it is not copied to `buf`.
    ///
    /// Returns a tuple `(count, found)` indicating how many non-matching bytes
    /// were consumed and the matching byte, if any.  Note that `count` can
    /// exceed `buf.len()`.
    ///
    /// On return, if a byte matched, the stream is positioned at the following
    /// byte. Otherwise, the stream is positioned at EOF.
    fn read_until_bits(&mut self, bits: u8, mask: u8, mut buf: &mut [u8])
        -> io::Result<(usize, Option<u8>)>
    {
        self.scan_until_bits(bits, mask, |bytes| {
            buf.write(bytes).unwrap();
        })
    }
}

impl<R: BufRead> BufReadExt for R {
    fn scan_until_bits<F>(&mut self, bits: u8, mask: u8, mut f: F)
        -> io::Result<(usize, Option<u8>)>
    where
        F: FnMut(&[u8])
    {
        let mut consumed = 0;

        // Until delimiter or EOF is found...
        loop {
            let (count, found) = {
                // Read Get next chunk from the stream
                let buf = match self.fill_buf() {
                    Ok(b) if b.len() == 0 /*EOF*/  => return Ok((consumed, None)),
                    Ok(b)                          => b,
                    Err(ref e) if e.is_transient() => continue,
                    Err(e)                         => return Err(e),
                };

                // Search chunk for delimiter with desired bit pattern
                // - If any bytes were skipped, invoke f with them
                // - If delimiter was found, include it in consumed bytes
                match buf.find_bits(bits, mask) {
                    Some((0, b)) => {               (    1,     Some(b)) },
                    Some((i, b)) => { f(&buf[..i]); (i + 1,     Some(b)) },
                    None         => { f( buf     ); (buf.len(), None   ) },
                }
            };

            // Mark bytes consumed
            self.consume(count);
            consumed += count;

            // Check if found
            if found.is_some() {
                return Ok((consumed, found))
            }
        }
    }
}

// Saved from prevous work:
//
//  /// Returns an unexpected-EOF error at the current offset.
//  fn unexpected_eof(&self) -> Error {
//      Error::new(
//          ErrorKind::UnexpectedEof,
//          format!("At offset {}: unexpected end of file.", self.offset)
//      )
//  }

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_u8() {
        //  index      0     1
        let bytes   = [0x12, 0x34];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u8().unwrap(), 0x12);
        assert_eq!(src.read_u8().unwrap(), 0x34);
        assert_eq!(src.read_u8().err().unwrap().kind(), UnexpectedEof);
    }

    #[test]
    fn read_u16() {
        //  index      0           1           -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u16().unwrap(), 0x1234);
        assert_eq!(src.read_u16().unwrap(), 0x5678);
        assert_eq!(src.read_u16().err().unwrap().kind(), UnexpectedEof);
    }

    #[test]
    fn read_u32() {
        //  index      0                       1                       -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0xA5];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u32().unwrap(), 0x12345678);
        assert_eq!(src.read_u32().unwrap(), 0x9ABCDEF0);
        assert_eq!(src.read_u32().err().unwrap().kind(), UnexpectedEof);
    }

    #[test]
    fn skip_until_bits_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.skip_until_bits(0x56, 0xFF).unwrap(), (3, Some(0x56)));
        assert_eq!(src.read_u8().unwrap(), 0x78);
    }

    #[test]
    fn skip_until_bits_not_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.skip_until_bits(0x0A, 0xFF).unwrap(), (4, None));
    }

    #[test]
    fn read_until_bits_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78];
        let mut src = Cursor::new(&bytes);
        let mut buf = [0; 2];

        assert_eq!(src.read_until_bits(0x56, 0xFF, &mut buf).unwrap(), (3, Some(0x56)));
        assert_eq!(src.read_u8().unwrap(), 0x78);
        assert_eq!(buf[0], 0x12);
        assert_eq!(buf[1], 0x34);
    }

    #[test]
    fn read_until_bits_not_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78];
        let mut src = Cursor::new(&bytes);
        let mut buf = [0; 2];

        assert_eq!(src.read_until_bits(0x0A, 0xFF, &mut buf).unwrap(), (4, None));
        assert_eq!(buf[0], 0x12);
        assert_eq!(buf[1], 0x34);
    }
}

