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

use a6::block::*;
use a6::error::BlockDecoderError;
use a6::error::BlockDecoderError::*;
use util::{BoolArray, Handler};

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
            image:       block_buffer(n),
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::BlockDecoderError::*;

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

