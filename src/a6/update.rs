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

use io::*;
use util::BoolArray;

use self::BlockDecoderError::*;

const BLOCK_HEAD_LEN:   usize =  16;  // Raw block header length (bytes)
const BLOCK_DATA_LEN:   usize = 256;  // Raw block data length (bytes)
const BLOCK_7BIT_LEN:   usize = 311;  // 7-bit-encoded block length (bytes)

// Maximum image size
const IMAGE_MAX_BYTES:  usize = 2 * 1024 * 1024;
const IMAGE_MAX_BLOCKS: usize = IMAGE_MAX_BYTES / BLOCK_DATA_LEN;

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
pub struct BlockDecoder<H: BlockDecoderHandler> {
    state:   Option<BlockDecoderState>,
    handler: H,
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

pub trait BlockDecoderHandler {
    fn on_err(&self, e: BlockDecoderError) -> bool;
}

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

impl<H> BlockDecoder<H> where H: BlockDecoderHandler {
    fn new(handler: H) -> Self {
        Self { state: None, handler }
    }

    fn consume_block(&mut self, mut block: &[u8]) -> Result<(), ()> {
        if block.len() != BLOCK_HEAD_LEN + BLOCK_DATA_LEN {
            let err = InvalidBlockLength { actual: block.len() };
            self.handler.on_err(err);
            return Err(());
        }

        let header = BlockHeader {
            version:     block.read_u32().unwrap(),
            checksum:    block.read_u32().unwrap(),
            length:      block.read_u32().unwrap(),
            block_count: block.read_u16().unwrap(),
            block_index: block.read_u16().unwrap(),
        };

        let state = self.check_state(header);

        state.write_block(header.block_index as usize, block);

        Ok(())
    }

    fn check_state(&mut self, header: BlockHeader) -> &mut BlockDecoderState {
        match self.state {
            Some(ref mut state) => {
                state
            },
            None => {
                let cnt = header.block_count as usize;
                let len = header.length      as usize;

                // ...

                self.state = Some(BlockDecoderState {
                    header,
                    blocks_done: BoolArray::new(cnt),
                    image:       vec![0; len].into_boxed_slice(),
                });
                self.state.as_mut().unwrap()
            },
        }
    }
}

impl BlockHeader {
    fn require_match(&self, other: &Self) -> Option<String> {
        if self.version != other.version {
            return Some("version mismatch".into())
        }
        if self.checksum != other.checksum {
            return Some("checksum mismatch".into())
        }
        if self.length != other.length {
            return Some("length mismatch".into())
        }
        if self.block_count != other.block_count {
            return Some("block count mismatch".into())
        }
        None
    }
}

impl BlockDecoderState {
    fn write_block(&mut self, index: usize, data: &[u8]) {
        let start = index * BLOCK_DATA_LEN;
        let end   = start + BLOCK_DATA_LEN;
        self.image[start..end].copy_from_slice(data);
        self.blocks_done.set(index);
    }
}

