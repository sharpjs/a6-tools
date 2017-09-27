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

use std::io::{BufRead, Cursor, Error, ErrorKind, Read, Result, Write};
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
            fn $name(&mut self) -> Result<$t> {
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
    fn scan_until_bits<F>(&mut self, bits: u8, mask: u8, each: F)
        -> Result<(usize, Option<u8>)>
    where
        F: FnMut(&[u8]);

    fn skip_until_bits(&mut self, bits: u8, mask: u8)
        -> Result<(usize, Option<u8>)>
    {
        self.scan_until_bits(bits, mask, |_| {})
    }

    fn read_until_bits(&mut self, bits: u8, mask: u8, buf: &mut [u8])
        -> Result<(usize, usize, Option<u8>)>
    {
        let mut cursor = Cursor::new(buf);

        let (count, byte) = self.scan_until_bits(bits, mask, |buf| {
            cursor.write(buf).unwrap();
        })?;

        Ok((count, cursor.position() as usize, byte))
    }
}

impl<R: BufRead> BufReadExt for R {
    fn scan_until_bits<F>(&mut self, bits: u8, mask: u8, mut f: F) -> Result<(usize, Option<u8>)>
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

/// A named input stream.
pub struct Input<R> {
    stream: R,
    offset: usize,
    name:   String,
}

impl<R: BufRead> Input<R> {
    /// Constructs a new `Input` with the given `stream` and `name`.
    ///
    /// Initial `offset` is `0`, regardless of the actual position of the given
    /// stream.
    #[inline]
    pub fn new<N: ToString>(stream: R, name: N) -> Self {
        Input {
            stream: stream,
            offset: 0,
            name:   name.to_string(),
        }
    }

    /// Returns the offset of the next unread byte in the input stream.
    #[inline(always)]
    pub fn offset(&self) -> usize { self.offset }

    /// Reads and discards bytes until the given `predicate` returns `true` for
    /// a byte (the byte *matches*), or until end-of-stream is reached.
    ///
    /// Returns a tuple `(found, count)` indicating whether a byte matched and
    /// how many non-matching bytes were discarded.
    ///
    /// On return, if a byte matched, the stream is positioned at that byte, so
    /// that the byte will be the next one read.  Otherwise, the stream is
    /// positioned at end-of-stream.
    pub fn skip_until<P: Fn(u8) -> bool>(&mut self, predicate: P) -> Result<(bool, usize)> {
        // Implementation similar to std::io::read_until().
        let mut total = 0;

        loop {
            let (found, count) = {
                // Get next chunk from the stream
                let buf = match self.stream.fill_buf() {
                    Ok(b) => b,
                    Err(ref e) if e.kind() == Interrupted => continue,
                    Err(e) => return Err(e),
                };

                // Search chunk for the delimiter
                match buf.iter().position(|&b| predicate(b)) {
                    Some(i) => (true,  i),
                    None    => (false, buf.len()),
                }
            };

            // Discard skipped bytes
            self.stream.consume(count);
            self.offset += count;
                 total  += count;

            // Check if done
            if found || count == 0 /*EOF*/ {
                return Ok((found, total));
            }
        }
    }

    pub fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
        self.stream.read_until(byte, buf)
    }

    /// Read the exact number of bytes required to fill `buf`.
    ///
    /// Same as `std::io::Read::read_exact()`, except that unexpected-EOF errors
    /// have improved messaging.
    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match self.stream.read_exact(buf) {
            Ok(_) => {
                // Advance position
                self.offset += buf.len();
                Ok(())
            },
            Err(ref e) if e.kind() == UnexpectedEof => {
                // Change error message
                Err(self.unexpected_eof())
            },
            e => e // Use error verbatim
        }
    }

    /// Returns an unexpected-EOF error at the current offset.
    fn unexpected_eof(&self) -> Error {
        Error::new(
            ErrorKind::UnexpectedEof,
            format!("At offset {}: unexpected end of file.", self.offset)
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};
    use super::*;

    #[test]
    fn read_u8() {
        //  index      0     1
        let bytes   = [0x12, 0x34];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u8().unwrap(), 0x12);
        assert_eq!(src.read_u8().unwrap(), 0x34);
        assert_eq!(src.read_u8().err().unwrap().kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_u16() {
        //  index      0           1           -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u16().unwrap(), 0x1234);
        assert_eq!(src.read_u16().unwrap(), 0x5678);
        assert_eq!(src.read_u16().err().unwrap().kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_u32() {
        //  index      0                       1                       -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0xA5];
        let mut src = Cursor::new(&bytes);

        assert_eq!(src.read_u32().unwrap(), 0x12345678);
        assert_eq!(src.read_u32().unwrap(), 0x9ABCDEF0);
        assert_eq!(src.read_u32().err().unwrap().kind(), ErrorKind::UnexpectedEof);
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
}

