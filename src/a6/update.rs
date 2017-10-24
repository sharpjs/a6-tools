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
#[repr(C, packed)]
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
#[repr(C, packed)]
pub struct Block {
    /// Metadata header.
    pub header: BlockHeader,

    /// Data payload.
    pub data: [u8; BLOCK_DATA_LEN],
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
    InvalidBlockCount       { actual: u16, expected: u16             },
    InconsistentVersion     { actual: u32, expected: u32, index: u16 },
    InconsistentChecksum    { actual: u32, expected: u32, index: u16 },
    InconsistentImageLength { actual: u32, expected: u32, index: u16 },
    InconsistentBlockCount  { actual: u16, expected: u16, index: u16 },
    ChecksumMismatch        { actual: u32, expected: u32             },
    MissingBlock            { count:  u16,                index: u16 },
}

impl<H> BlockDecoder<H> where H: Handler<BlockDecoderError> {
    /// Creates a `BlockDecoder` with the given `capacity` and `handler`.
    pub fn new(capacity: u32, handler: H) -> Self {
        if capacity > IMAGE_MAX_BYTES {
            panic!(
                "Capacity {} is beyond the supported range of 0 to {} bytes.",
                capacity, IMAGE_MAX_BYTES
            );
        }
        Self { state: None, capacity, handler }
    }

    /// Decodes the given `block`, adding its data to the image in progress.
    pub fn decode_block(&mut self, mut block: &[u8]) -> Result<(), ()> {
        // Validate block length
        if block.len() != BLOCK_HEAD_LEN + BLOCK_DATA_LEN {
            self.handler.on(&InvalidBlockLength {
                actual: block.len()
            });
            return Err(());
        }

        // Read block header, leaving `block` to contain just the data
        let header = BlockHeader {
            version:     block.read_u32().unwrap(),
            checksum:    block.read_u32().unwrap(),
            length:      block.read_u32().unwrap(),
            block_count: block.read_u16().unwrap(),
            block_index: block.read_u16().unwrap(),
        };

        // Check block header
        let state = self.check_state(header)?;

        // Write block data
        state.write_block(header.block_index, block);
        Ok(())
    }

    /// Validates and returns the decoded image.
    pub fn image(&self) -> Result<&[u8], ()> {
        // Verify that first block was decoded
        let state = match self.state {
            None => {
                self.handler.on(&MissingBlock {
                    count: 1, // TODO: Actual count?
                    index: 0,
                })?;
                return Ok(&[])
            },
            Some(ref state) => state,
        };

        // Check for missing blocks
        if let Some(n) = state.first_missing_block() {
            self.handler.on(&MissingBlock {
                count: 1, // TODO: Actual count?
                index: n,
            })?;
        }

        // Validate checksum
        let image = &state.image[..state.header.length as usize];
        let sum   = checksum(image);
        if sum != state.header.checksum {
            self.handler.on(&ChecksumMismatch {
                actual:   sum,
                expected: state.header.checksum,
            })?;
        }

        Ok(image)
    }

    fn check_state(&mut self, header: BlockHeader) -> Result<&mut BlockDecoderState, ()> {
        match self.state {
            None => {
                // Initialize state from first block's header
                self.init_state(header)
            },
            Some(ref mut state) => {
                // Check that block's header matches the first block's header
                if let Ok(_) = header.check_match(&state.header, &self.handler) {
                    Ok(state)
                } else {
                    Err(())
                }
            },
        }
    }

    /// Initializes decoder state using the given `header`.
    fn init_state(&mut self, header: BlockHeader) -> Result<&mut BlockDecoderState, ()> {
        // Validate header
        header.check_len(&self.handler)?;

        // Initialize decoder state
        self.state = Some(BlockDecoderState {
            header,
            blocks_done: BoolArray::new(header.block_count as usize),
            image:       vec![0; header.length as usize].into_boxed_slice(),
        });

        // Return mutable ref to state
        Ok(self.state.as_mut().unwrap())
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

        return Ok(());
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
        &*self.image
    }

    #[inline]
    fn has_block(&self, index: u16) -> bool {
        self.blocks_done.get(index as usize)
    }

    #[inline]
    fn first_missing_block(&self) -> Option<u16> {
        self.blocks_done.first_false().map(|v| v as u16)
    }

    fn write_block(&mut self, index: u16, data: &[u8]) {
        self.image[block_range(index)].copy_from_slice(data);
        self.blocks_done.set(index as usize);
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
                f, "Invalid block length: {} byte(s). Blocks must be exactly {} bytes long.",
                actual, BLOCK_HEAD_LEN + BLOCK_DATA_LEN,
            ),
            InvalidImageLength { actual } => write!(
                f, "Invalid image length: {} byte(s). The maximum image length is {} bytes.",
                actual, IMAGE_MAX_BYTES,
            ),
            InvalidBlockCount { actual, expected } => write!(
                f, "Invalid block count: {} block(s). The image length requires {} blocks.",
                actual, expected,
            ),
            InconsistentVersion { actual, expected, index } => write!(
                f, "Block {}: inconsistent version: {:X}. The initial block specified version {:X}.",
                index, actual, expected
            ),
            InconsistentChecksum { actual, expected, index } => write!(
                f, "Block {}: inconsistent checksum: {:X}. The initial block specified checksum {:X}.",
                index, actual, expected
            ),
            InconsistentImageLength { actual, expected, index } => write!(
                f, "Block {}: inconsistent image length: {} byte(s). The initial block specified a length of {} byte(s).",
                index, actual, expected
            ),
            InconsistentBlockCount { actual, expected, index } => write!(
                f, "Block {}: inconsistent block count: {} block(s). The initial block specified a count of {} block(s).",
                index, actual, expected
            ),
            ChecksumMismatch { actual, expected } => write!(
                f, "Computed checksum {:X} does not match checksum {:X} specified in block headers.",
                actual, expected
            ),
            MissingBlock { count, index } => write!(
                f, "Incomplete image: {} missing block(s). First missing block is at index {} (0-based).",
                count, index
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_state() -> BlockDecoderState {
        BlockDecoderState::new(BlockHeader {
            version:     0, // don't care
            checksum:    0, // don't care
            length:      0, // don't care
            block_count: 4,
            block_index: 0, // don't care
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
        let image = &[0; 4 * BLOCK_DATA_LEN][..];

        assert_eq!(state.image(), image);
        assert_eq!(state.first_missing_block(), Some(0));
        assert_eq!(state.first_missing_block(), Some(0));
    }

    #[test]
    fn state_after_write_at0() {
        let mut state = new_state();
        let     block = &    [0xA5;     BLOCK_DATA_LEN][..];
        let     image = &mut [0x00; 4 * BLOCK_DATA_LEN][..];

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
        let     block = &    [0xA5;     BLOCK_DATA_LEN][..];
        let     image = &mut [0x00; 4 * BLOCK_DATA_LEN][..];

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
        let     block = &    [0xA5;     BLOCK_DATA_LEN][..];
        let     image = &mut [0x00; 4 * BLOCK_DATA_LEN][..];

        image[  0.. 256].copy_from_slice(block);
        image[256.. 512].copy_from_slice(block);
        image[512.. 768].copy_from_slice(block);
        image[768..1024].copy_from_slice(block);

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

