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

use std::io::{BufRead, Result, Error, ErrorKind};
use std::io::ErrorKind::{Interrupted, UnexpectedEof};

pub struct Input<R> {
    stream: R,
    offset: usize,
    name:   String,
}

impl<R: BufRead> Input<R> {
    pub fn new<N: ToString>(stream: R, name: N) -> Self {
        Input {
            stream: stream,
            offset: 0,
            name:   name.to_string(),
        }
    }

    /// Skips bytes until the delimiter `byte` or EOF is found.
    ///
    /// This function reads bytes from the underlying stream until the delimiter
    /// or end-of-file is found.  The read bytes, including the delimitar, are
    /// discarded.
    ///
    /// This function returns a tuple indicating whether the delimitar was found
    /// and how many bytes were discarded.
    pub fn skip_until(&mut self, byte: u8) -> Result<(bool, usize)> {
        // Like read_until, but throws away read bytes.
        // Returns 
        loop {
            let (found, count) = {
                let buf = match self.stream.fill_buf() {
                    Ok(b) => b,
                    Err(ref e) if e.kind() == Interrupted => continue,
                    Err(e) => return Err(e),
                };
                match buf.iter().position(|b| *b == byte) {
                    Some(i) => (true, i),
                    None    => (false, buf.len()),
                }
            };

            self.stream.consume(count);
            self.offset += count;

            if found || count == 0 {
                // This isn't right.  Fix it.
                return Ok((found, count));
            }
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
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

    fn unexpected_eof(&self) -> Error {
        Error::new(
            ErrorKind::UnexpectedEof,
            format!("At offset {}: unexpected end of file.", self.offset)
        )
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
            pub fn $name(&mut self) -> Result<$t> {
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

impl<R: BufRead> Input<R> {
    def_read! {
        read_u8  (1, v: u8 ) { v         }
        read_u16 (2, v: u16) { v.to_be() }
        read_u32 (4, v: u32) { v.to_be() }
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
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.read_u8().unwrap(), 0x12);
        assert_eq!(src.read_u8().unwrap(), 0x34);
        assert_eq!(src.read_u8().err().unwrap().kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_u16() {
        //  index      0           1           -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.read_u16().unwrap(), 0x1234);
        assert_eq!(src.read_u16().unwrap(), 0x5678);
        assert_eq!(src.read_u16().err().unwrap().kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_u32() {
        //  index      0                       1                       -
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0xA5];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.read_u32().unwrap(), 0x12345678);
        assert_eq!(src.read_u32().unwrap(), 0x9ABCDEF0);
        assert_eq!(src.read_u32().err().unwrap().kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn skip_until_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.skip_until(0x56).unwrap(), (true, 2));
        assert_eq!(src.read_u8().unwrap(), 0x56);
    }
}

