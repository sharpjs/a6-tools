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

use std::mem::size_of;
use io::*;
use util::BoolArray;

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
pub struct BlockDecoder {
    state: Option<BlockDecoderState>,
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

impl BlockDecoder {
    fn new() -> Self {
        Self { state: None }
    }

    fn consume_block(&mut self, mut block: &[u8]) -> Result<(), ()> {
        if block.len() != size_of::<Block>() {
            return Err(())
        }

        let header = BlockHeader {
            version:     block.read_u32().unwrap(),
            checksum:    block.read_u32().unwrap(),
            length:      block.read_u32().unwrap(),
            block_count: block.read_u16().unwrap(),
            block_index: block.read_u16().unwrap(),
        };

        let state = self.check_state(header);
        let data  = block;

        // ...

        Ok(())
    }

    fn check_state(&mut self, header: BlockHeader) -> &mut BlockDecoderState {
        match self.state {
            Some(ref mut state) => state,
            None => {
                let map_len = header.block_count as usize;
                let img_len = header.length      as usize;

                // ...

                self.state = Some(BlockDecoderState {
                    header,
                    blocks_done: BoolArray::new(map_len),
                    image:       vec![0; img_len].into_boxed_slice(),
                });
                self.state.as_mut().unwrap()
            },
        }
    }
}

