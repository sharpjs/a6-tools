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

use a6::update::{BLOCK_HEAD_LEN, BLOCK_DATA_LEN, IMAGE_MAX_BYTES, IMAGE_MAX_BLOCKS};

use self::BlockDecoderError::*;

/// Error conditions reportable during block decoding.
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

