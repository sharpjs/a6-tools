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

use std::ops::Range;

use a6::error::BlockDecodeError;
use a6::error::BlockDecodeError::*;
use io::*;
use util::Handler;

pub const BLOCK_HEAD_LEN:   usize =  16;  // Raw block header length (bytes)
pub const BLOCK_DATA_LEN:   usize = 256;  // Raw block data length (bytes)
pub const BLOCK_7BIT_LEN:   usize = 311;  // 7-bit-encoded block length (bytes)

// Maximum image size
pub const IMAGE_MAX_BYTES:  u32 = 2 * 1024 * 1024;
pub const IMAGE_MAX_BLOCKS: u16 = (IMAGE_MAX_BYTES as usize / BLOCK_DATA_LEN) as u16;

const BLOCK_DIV_SHIFT:  usize = 8;

/// Metadata describing a bootloader/OS update block.
#[derive(Clone, Copy, Debug)]
pub struct BlockHeader {
    /// Version of the firmware in the image.
    pub version: u32,

    /// Checksum of the image.
    pub checksum: u32,

    /// Length of the image.
    pub length: u32,

    /// Count of 256-byte blocks in the image.
    pub block_count: u16,

    /// 0-based index of the block.
    pub block_index: u16,
}

/// A portion of an OS/bootloader update image.
#[derive(Clone, Copy, Debug)]
pub struct Block<'a> {
    /// Metadata header.
    pub header: BlockHeader,

    /// Data payload.
    pub data: &'a [u8],
}

impl<'a> Block<'a> {
    /// Creates a `Block` from the given `bytes`, reporting problems to the
    /// given `handler`.
    ///
    /// Returns the block if `bytes` is exactly block-sized or if `bytes` is too
    /// large and `handler` returns `Ok(())` (continue).
    ///
    /// Returns `Err(true)` if `bytes` is too small and `handler` returns
    /// `Ok(())` (continue).
    ///
    /// Returns `Err(false) if `bytes` is too small or too large and `handler`
    /// returns `Err(())` (stop).
    pub fn from_bytes<H>(mut bytes: &'a [u8], handler: &H) -> Result<Self, bool>
        where H: Handler<BlockDecodeError>
    {
        const LEN: usize = BLOCK_HEAD_LEN + BLOCK_DATA_LEN;

        // Validate block length
        if bytes.len() != LEN {
            // Notify handler of bad length; allow handler to abort
            handler
                .on(&InvalidBlockLength { actual: bytes.len() })
                .or(Err(false))?;

            // Not aborting; check if there are enough bytes
            bytes = match bytes.get(..LEN) {
                Some(b) => b,
                None    => return Err(true),
            };
        }

        // Read block header, leaving `bytes` to contain just the data
        let header = BlockHeader {
            version:     bytes.read_u32().unwrap(),
            checksum:    bytes.read_u32().unwrap(),
            length:      bytes.read_u32().unwrap(),
            block_count: bytes.read_u16().unwrap(),
            block_index: bytes.read_u16().unwrap(),
        };

        // Create block
        Ok(Self { header, data: bytes })
    }
}

impl BlockHeader {
    /// Verifies that the header specifies a valid image length and block count.
    pub fn check_len<H>(&self, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecodeError>
    {
        // Validate claimed image length
        if self.length > IMAGE_MAX_BYTES {
            handler.on(&InvalidImageLength {
                actual: self.length,
            });
            return Err(());
        }

        // Cannot fall through here, because `self.length` is potentially out
        // of the limited domain of block_count_for().

        // Validate claimed block count
        let bc = block_count_for(self.length);
        if self.block_count != bc {
            handler.on(&InvalidBlockCount {
                actual:   self.block_count,
                expected: bc,
            });
            return Err(());
        }

        Ok(())
    }

    /// Verifies that the header's fields (except `block_index`) match those of
    /// the given `other` header.
    pub fn check_match<H>(&self, other: &BlockHeader, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecodeError>
    {
        let mut result = Ok(());

        if self.version != other.version {
            handler.on(&InconsistentVersion {
                actual:   self .version,
                expected: other.version,
                index:    self .block_index,
            })?;
            result = Err(());
        }

        if self.checksum != other.checksum {
            handler.on(&InconsistentChecksum {
                actual:   self .checksum,
                expected: other.checksum,
                index:    self .block_index,
            })?;
            result = Err(());
        }

        if self.length != other.length {
            handler.on(&InconsistentImageLength {
                actual:   self .length,
                expected: other.length,
                index:    self .block_index,
            })?;
            result = Err(());
        }

        if self.block_count != other.block_count {
            handler.on(&InconsistentBlockCount {
                actual:   self .block_count,
                expected: other.block_count,
                index:    self .block_index,
            })?;
            result = Err(());
        }

        result
    }

    /// Verifies that the header specifies a valid block index.
    pub fn check_block_index<H>(&self, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecodeError>
    {
        if self.block_index >= self.block_count {
            handler.on(&InvalidBlockIndex {
                actual: self.block_index,
                max:    self.block_count.saturating_sub(1),
            });
        }

        Ok(())
    }
}

#[inline]
fn block_count_for(len: u32) -> u16 {
    // Ceiling of `len` divided by `BLOCK_DATA_LEN`
    match len {
        0 => 0,
        n => 1 + (n - 1 >> BLOCK_DIV_SHIFT) as u16
    }
}

#[inline]
pub fn block_buffer(count: usize) -> Box<[u8]> {
    vec![0; count << BLOCK_DIV_SHIFT].into_boxed_slice()
}

#[inline]
pub fn block_range(index: u16) -> Range<usize> {
    let index = index as usize;
    let start = index << BLOCK_DIV_SHIFT;
    let end   = start  + BLOCK_DATA_LEN;
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::BlockDecodeError::*;

    struct Panicker;

    impl Handler<BlockDecodeError> for Panicker {
        fn on(&self, event: &BlockDecodeError) -> Result<(), ()> {
            panic!("Unexpected event: {:?}", event)
        }
    }

    impl Handler<BlockDecodeError> for Vec<(BlockDecodeError, Result<(), ()>)> {
        fn on(&self, event: &BlockDecodeError) -> Result<(), ()> {
            match self.iter().find(|&&(e, _)| e == *event) {
                Some(&(_, result)) => result,
                None               => panic!("Unexpected event: {:?}", event),
            }
        }
    }

    #[test]
    fn block_from_bytes_ok() {
        let bytes
            = (0..0x010)        // header
            .chain(0..0x100)    // data
            .map(|x| x as u8)
            .collect::<Vec<_>>();

        let block = Block::from_bytes(&bytes[..], &vec![]).unwrap();

        assert_eq!(block.header.version,     0x00010203);
        assert_eq!(block.header.checksum,    0x04050607);
        assert_eq!(block.header.length,      0x08090A0B);
        assert_eq!(block.header.block_count, 0x0C0D);
        assert_eq!(block.header.block_index, 0x0E0F);
    }

    #[test]
    fn block_from_bytes_too_few_continue() {
        let bytes = vec![0; 42];

        let handler = vec![
            ( InvalidBlockLength { actual: bytes.len() }, Ok(()) )
        ];

        let result = Block::from_bytes(&bytes[..], &handler);

        assert_eq!(result.unwrap_err(), true);
    }

    #[test]
    fn block_from_bytes_too_few_abort() {
        let bytes = vec![0; 42];

        let handler = vec![
            ( InvalidBlockLength { actual: bytes.len() }, Err(()) )
        ];

        let result = Block::from_bytes(&bytes[..], &handler);

        assert_eq!(result.unwrap_err(), false);
    }

    #[test]
    fn block_from_bytes_too_many_continue() {
        let bytes
            = (0..0x010)        // header
            .chain(0..0x100)    // data
            .chain(0..1)        // extra byte
            .map(|x| x as u8)
            .collect::<Vec<_>>();

        let handler = vec![
            ( InvalidBlockLength { actual: bytes.len() }, Ok(()) )
        ];

        let block = Block::from_bytes(&bytes[..], &handler).unwrap();

        assert_eq!(block.header.version,     0x00010203);
        assert_eq!(block.header.checksum,    0x04050607);
        assert_eq!(block.header.length,      0x08090A0B);
        assert_eq!(block.header.block_count, 0x0C0D);
        assert_eq!(block.header.block_index, 0x0E0F);
    }

    #[test]
    fn block_from_bytes_too_many_abort() {
        let bytes
            = (0..0x010)        // header
            .chain(0..0x100)    // data
            .chain(0..1)        // extra byte
            .map(|x| x as u8)
            .collect::<Vec<_>>();

        let handler = vec![
            ( InvalidBlockLength { actual: bytes.len() }, Err(()) )
        ];

        let result = Block::from_bytes(&bytes[..], &handler);

        assert_eq!(result.unwrap_err(), false);
    }
}

