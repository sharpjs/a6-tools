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

use std::fmt;
use std::mem::size_of;
use std::ops::Range;

use io::*;
use util::{BoolArray, Handler};

use self::BlockDecoderError::*;

const BLOCK_HEAD_LEN:   usize =  16;  // Raw block header length (bytes)
const BLOCK_DATA_LEN:   usize = 256;  // Raw block data length (bytes)
const BLOCK_7BIT_LEN:   usize = 311;  // 7-bit-encoded block length (bytes)

const BLOCK_DIV_SHIFT:  usize = 8;
const BLOCK_REM_MASK:   usize = (1 << BLOCK_DIV_SHIFT) - 1;

// Maximum image size
const IMAGE_MAX_BYTES:  u32 = 2 * 1024 * 1024;
const IMAGE_MAX_BLOCKS: u16 = (IMAGE_MAX_BYTES as usize / BLOCK_DATA_LEN) as u16;

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

#[derive(Clone)]
struct BlockDecoderState {
    /// Block 0 metadata.
    header: BlockHeader,

    /// Map of 'done' bits for each block.
    blocks_done: BoolArray,

    /// Buffer for image in progress.
    image: Box<[u8]>,
}

/// Constructs a binary image from A6 OS/bootloader update blocks.
#[derive(Clone)]
pub struct BlockDecoder<H> where H: Handler<BlockDecoderError> {
    /// Current state, populated on first block.
    state: Option<BlockDecoderState>,

    /// Maximum image size.
    capacity: u32,

    /// Handler for error conditions.
    handler: H,
}

/// Error conditions reportable by `BlockDecoder`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockDecoderError {
    InvalidBlockLength      { actual: usize                          },
    InvalidImageLength      { actual: u32                            },
    InvalidBlockIndex       { actual: u16, max: u16                  },
    InvalidBlockCount       { actual: u16, expected: u16             },
    InconsistentVersion     { actual: u32, expected: u32, index: u16 },
    InconsistentChecksum    { actual: u32, expected: u32, index: u16 },
    InconsistentImageLength { actual: u32, expected: u32, index: u16 },
    InconsistentBlockCount  { actual: u16, expected: u16, index: u16 },
    ChecksumMismatch        { actual: u32, expected: u32             },
    DuplicateBlock          {                             index: u16 },
    MissingBlock            {                             index: u16 },
}

impl<H> BlockDecoder<H> where H: Handler<BlockDecoderError> {
    /// Creates a `BlockDecoder` with the given `capacity` and `handler`.
    pub fn new(capacity: u32, handler: H) -> Self {
        if capacity > IMAGE_MAX_BYTES {
            panic!(
                "Capacity {} is beyond the supported maximum of {} bytes.",
                capacity, IMAGE_MAX_BYTES
            );
        }
        Self { state: None, capacity, handler }
    }

    /// Decodes the given `block`, adding its data to the image in progress.
    pub fn decode_block(&mut self, block: &[u8]) -> Result<(), ()> {
        // Read block
        let block = match Block::from_bytes(block, &self.handler) {
            Ok(b)      => b,
            Err(true)  => return Ok(()),    // continue
            Err(false) => return Err(()),   // abort
        };

        // Check block header
        let state = match self.state {
            None => {
                // Initialize decoder state from first block header
                block.header.check_len(&self.handler)?;
                self.state = Some(BlockDecoderState::new(block.header));
                self.state.as_mut().unwrap()
            },
            Some(ref mut state) => {
                // Check that block's header matches the first block's header
                block.header.check_match(&state.header, &self.handler)?;
                state
            },
        };

        // Write block data
        if state.write_block(block.header.block_index, block.data) {
            self.handler.on(&DuplicateBlock {
                index: block.header.block_index,
            })?;
        }

        Ok(())
    }

    /// Validates and returns the decoded image.
    pub fn image(&self) -> Result<&[u8], ()> {
        // Verify that first block was decoded
        let state = match self.state {
            None => {
                self.handler.on(&MissingBlock { index: 0 })?;
                return Ok(&[])
            },
            Some(ref state) => state,
        };

        // Check for missing blocks
        if let Some(n) = state.first_missing_block() {
            self.handler.on(&MissingBlock { index: n })?;
        }

        // Validate checksum
        let image = state.image();
        let sum   = checksum(image);
        if sum != state.header.checksum {
            self.handler.on(&ChecksumMismatch {
                actual:   sum,
                expected: state.header.checksum,
            })?;
        }

        Ok(image)
    }
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
    fn from_bytes<H>(mut bytes: &'a [u8], handler: &H) -> Result<Self, bool>
        where H: Handler<BlockDecoderError>
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
    fn check_len<H>(&self, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecoderError>
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
    fn check_match<H>(&self, other: &BlockHeader, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecoderError>
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
    fn check_block_index<H>(&self, handler: &H) -> Result<(), ()>
        where H: Handler<BlockDecoderError>
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

fn checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for &b in bytes {
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

impl BlockDecoderState {
    fn new(header: BlockHeader) -> Self {
        let n = header.block_count as usize;
        Self {
            header,
            blocks_done: BoolArray::new(n),
            image:       vec![0; n << BLOCK_DIV_SHIFT].into_boxed_slice(),
        }
    }

    #[inline]
    fn image(&self) -> &[u8] {
        &self.image[..self.header.length as usize]
    }

    #[inline]
    fn has_block(&self, index: u16) -> bool {
        self.blocks_done.get(index as usize)
    }

    #[inline]
    fn first_missing_block(&self) -> Option<u16> {
        self.blocks_done.first_false().map(|v| v as u16)
    }

    /// Writes the given block `data` at the given block `index`.  Returns `true`
    /// if the block has been written already, or `false` otherwise.
    fn write_block(&mut self, index: u16, data: &[u8]) -> bool {
        self.image[block_range(index)].copy_from_slice(data);
        self.blocks_done.set(index as usize)
    }
}

#[inline]
fn block_range(index: u16) -> Range<usize> {
    let index = index as usize;
    let start = index << BLOCK_DIV_SHIFT;
    let end   = start  + BLOCK_DATA_LEN;
    start..end
}

impl fmt::Display for BlockDecoderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            InvalidBlockLength { actual } => write!(
                f, "Invalid block length: {} byte(s). \
                    Blocks must be exactly {} bytes long ({} header bytes, {} data bytes).",
                actual, BLOCK_HEAD_LEN + BLOCK_DATA_LEN, BLOCK_HEAD_LEN, BLOCK_DATA_LEN,
            ),
            InvalidImageLength { actual } => write!(
                f, "Invalid image length: {} byte(s). \
                    The maximum image length is {} bytes.",
                actual, IMAGE_MAX_BYTES,
            ),
            InvalidBlockCount { actual, expected } => write!(
                f, "Invalid block count: {} block(s). \
                    This image requires {} blocks.",
                actual, expected,
            ),
            InvalidBlockIndex { actual, max } => write!(
                f, "Invalid block index: {}. \
                    The maximum for this image is {}.",
                actual, max,
            ),
            InconsistentVersion { actual, expected, index } => write!(
                f, "Block {}: inconsistent version: {:X}. \
                    The initial block specified version {:X}.",
                index, actual, expected
            ),
            InconsistentChecksum { actual, expected, index } => write!(
                f, "Block {}: inconsistent checksum: {:X}. \
                    The initial block specified checksum {:X}.",
                index, actual, expected
            ),
            InconsistentImageLength { actual, expected, index } => write!(
                f, "Block {}: inconsistent image length: {} byte(s). \
                    The initial block specified a length of {} byte(s).",
                index, actual, expected
            ),
            InconsistentBlockCount { actual, expected, index } => write!(
                f, "Block {}: inconsistent block count: {} block(s). \
                    The initial block specified a count of {} block(s).",
                index, actual, expected
            ),
            ChecksumMismatch { actual, expected } => write!(
                f, "Computed checksum {:X} does not match checksum {:X} specified in block headers.",
                actual, expected
            ),
            DuplicateBlock { index } => write!(
                f, "Block {}: duplicate block.",
                index
            ),
            MissingBlock { index } => write!(
                f, "Incomplete image: one or more block(s) is missing. \
                    First missing block is at index {}.",
                index
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::BlockDecoderError::*;

    struct Panicker;

    impl Handler<BlockDecoderError> for Panicker {
        fn on(&self, event: &BlockDecoderError) -> Result<(), ()> {
            panic!("Unexpected event: {:?}", event)
        }
    }

    impl Handler<BlockDecoderError> for Vec<(BlockDecoderError, Result<(), ()>)> {
        fn on(&self, event: &BlockDecoderError) -> Result<(), ()> {
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
    fn block_from_bytes_too_few_abort() {
        let bytes = vec![0; 42];

        let handler = vec![
            ( InvalidBlockLength { actual: bytes.len() }, Err(()) )
        ];

        let result = Block::from_bytes(&bytes[..], &handler);

        assert_eq!(result.unwrap_err(), false);
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

    fn new_state() -> BlockDecoderState {
        BlockDecoderState::new(BlockHeader {
            version:        0, // don't care
            checksum:       0, // don't care
            length:      1000, // \_ Test with image not using
            block_count:    4, // /    all of final block.
            block_index:    0, // don't care
        })
    }

    #[test]
    fn block_range_fn() {
        assert_eq!( block_range(    0),        0 ..      256 );
        assert_eq!( block_range(    3),      768 ..     1024 );
        assert_eq!( block_range(65535), 16776960 .. 16777216 );
    }

    #[test]
    fn state_initial() {
        let state = new_state();
        let image = &[0; 1000][..];

        assert_eq!(state.image(), image);
        assert_eq!(state.has_block(0), false);
        assert_eq!(state.has_block(1), false);
        assert_eq!(state.has_block(2), false);
        assert_eq!(state.has_block(3), false);
        assert_eq!(state.first_missing_block(), Some(0));
    }

    #[test]
    fn state_after_write_at0() {
        let mut state = new_state();
        let     block = &    [0xA5; BLOCK_DATA_LEN][..];
        let     image = &mut [0x00;           1000][..];

        image[0..256].copy_from_slice(block);

        state.write_block(0, block);

        assert_eq!(state.image(), image);
        assert_eq!(state.has_block(0), true);
        assert_eq!(state.has_block(1), false);
        assert_eq!(state.has_block(2), false);
        assert_eq!(state.has_block(3), false);
        assert_eq!(state.first_missing_block(), Some(1));
    }

    #[test]
    fn state_after_write_at2() {
        let mut state = new_state();
        let     block = &    [0xA5; BLOCK_DATA_LEN][..];
        let     image = &mut [0x00;           1000][..];

        image[512..768].copy_from_slice(block);

        state.write_block(2, block);

        assert_eq!(state.image(), image);
        assert_eq!(state.has_block(0), false);
        assert_eq!(state.has_block(1), false);
        assert_eq!(state.has_block(2), true);
        assert_eq!(state.has_block(3), false);
        assert_eq!(state.first_missing_block(), Some(0));
    }

    #[test]
    fn state_after_write_all() {
        let mut state = new_state();
        let     block = &    [0xA5; BLOCK_DATA_LEN][..];
        let     image = &mut [0x00;           1000][..];

        image[  0.. 256].copy_from_slice(block);
        image[256.. 512].copy_from_slice(block);
        image[512.. 768].copy_from_slice(block);
        image[768..1000].copy_from_slice(&block[..(1000-768)]);

        state.write_block(2, block); // out of order
        state.write_block(0, block);
        state.write_block(1, block);
        state.write_block(3, block);

        assert_eq!(state.image(), image);
        assert_eq!(state.has_block(0), true);
        assert_eq!(state.has_block(1), true);
        assert_eq!(state.has_block(2), true);
        assert_eq!(state.has_block(3), true);
        assert_eq!(state.first_missing_block(), None);
    }
}

