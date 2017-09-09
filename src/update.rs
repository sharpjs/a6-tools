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

const BLOCK_RAW_HEAD_LEN: usize =  16;  // Raw   block header  length (bytes)
const BLOCK_RAW_DATA_LEN: usize = 256;  // Raw   block data    length (bytes)
const BLOCK_MESSAGE_LEN:  usize = 311;  // SysEx block message length (bytes)

// TODO: This should be in a different file now.
/// A portion of a bootloader or operating system update image.
#[repr(C, packed)]
pub struct Block {
    /// Version of the software of which this block is a part.
    pub version: u32,

    /// Checksum of the binary of which this block is a part.
    pub checksum: u32,

    /// Length of the binary of which this block is a part.
    pub length: u32,

    /// Count of 256-byte blocks in this image.
    pub block_count: u16,

    /// 0-based index of this block.
    pub block_index: u16,

    /// Data payload of this block.
    pub data: [u8; BLOCK_RAW_DATA_LEN],
}

