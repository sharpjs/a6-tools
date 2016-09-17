// This file is part of a6-tools.
// Copyright (C) 2016 Jeffrey Sharp
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

use std::io::{BufRead, Result};
use std::io::ErrorKind::Interrupted;

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

    pub fn skip_until(&mut self, byte: u8) -> Result<bool> {
        // Like read_until, but throws away read bytes.
        loop {
            let (found, count) = {
                let buf = match self.stream.fill_buf() {
                    Ok(b) => b,
                    Err(ref e) if e.kind() == Interrupted => continue,
                    Err(e) => return Err(e),
                };
                match buf.iter().position(|b| *b == byte) {
                    Some(i) => (true, i),
                    None => (false, buf.len()),
                }
            };

            self.stream.consume(count);
            self.offset += count;

            if found || count == 0 {
                return Ok(found);
            }
        }
    }

    pub fn read_u8(&mut self) -> Result<Option<u8>> {
        let mut buf = [0; 1];
        match self.stream.read(&mut buf) {
            Ok  (1) => {},
            Ok  (_) => return Ok(None),
            Err (e) => return Err(e),
        }
        self.offset += 1;
        Ok(Some(buf[0]))
    }

    pub fn read_u16(&mut self) -> Result<Option<u16>> {
        let mut buf = [0; 2];
        match self.stream.read(&mut buf) {
            Ok  (2) => {},
            Ok  (_) => return Ok(None),
            Err (e) => return Err(e),
        }
        self.offset += 2;
        let val = ((buf[0] as u16) << 8) + (buf[1] as u16);
        Ok(Some(val))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use super::*;

    #[test]
    fn read_u8() {
        let bytes   = [0x12, 0x34];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.read_u8().unwrap(), Some(0x12));
        assert_eq!(src.read_u8().unwrap(), Some(0x34));
        assert_eq!(src.read_u8().unwrap(), None);
    }

    #[test]
    fn read_u16() {
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.read_u16().unwrap(), Some(0x1234));
        assert_eq!(src.read_u16().unwrap(), Some(0x5678));
        assert_eq!(src.read_u16().unwrap(), None);
    }

    #[test]
    fn skip_until_found() {
        let bytes   = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let stream  = Cursor::new(&bytes);
        let mut src = Input::new(stream, "test");

        assert_eq!(src.skip_until(0x56).unwrap(), true);
        assert_eq!(src.read_u8().unwrap(), Some(0x56));
    }
}

