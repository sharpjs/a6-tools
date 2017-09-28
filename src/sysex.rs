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

use std::io;
use std::io::prelude::*;
use io::*;
use self::SysExReadError::*;

// MIDI byte ranges
const MIDI_DATA_MIN:     u8 = 0x00; // \_ Data bytes
const MIDI_DATA_MAX:     u8 = 0x7F; // / 
const MIDI_STATUS_MIN:   u8 = 0x80; // \_ Status bytes
const MIDI_STATUS_MAX:   u8 = 0xEF; // /
const MIDI_SYSEX_START: u8 = 0xF0; // \_ System exlusive messages
const MIDI_SYSEX_END:   u8 = 0xF7; // /
const MIDI_SYSCOM_MIN:  u8 = 0xF1; // \_ System common messages
const MIDI_SYSCOM_MAX:  u8 = 0xF6; // /
const MIDI_SYSRT_MIN:   u8 = 0xF8; // \_ System real-time messages
const MIDI_SYSRT_MAX:   u8 = 0xFF; // /

/// Consumes the given `input` stream and detects MIDI System Exclusive messages
/// of length `cap` or less.  Invokes the handler `on_msg` for each detected
/// message and the handler `on_err` for each error condition.
pub fn read_sysex<R, M, E>(
    input:  &mut R,
    cap:    usize,
    on_msg: M,
    on_err: E,
)   ->      io::Result<bool>
where
    R: BufRead,
    M: Fn(usize, &[u8])                 -> bool,
    E: Fn(usize, usize, SysExReadError) -> bool,
{
    let mut pos = 0;
    let mut msg = vec![0u8; 0];

    loop {
        // State A: Not In SysEx Message
        {
            let (read, found) = input.skip_until_bits(MIDI_SYSEX_START, 0xFF)?;

            let len = match found {
                Some(_) => read - 1,
                None    => read,
            };

            if len != 0 && !on_err(pos, len, NotSysEx) {
                return Ok(false)
            }

            match found {
                Some(_) => pos += read,
                None    => return Ok(true),
            }
        }

        // State B: In SysEx Message
        loop {
            break
        }
    }

    Ok(true)
}

/// Possible error conditions encountered by `read_sysex`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysExReadError {
    /// The bytes did not contain a System Exclusive message.
    NotSysEx,

    /// A System Exclusive message exceeded the maximum allowed length.
    Overflow,

    /// A System Exclusive message was interrupted by an unexpected byte.
    UnexpectedByte,

    /// A System Exclusive message was interrupted by end-of-file.
    UnexpectedEof,
}

/// Encodes a sequence of bytes into a sequence of 7-bit values.
pub fn encode_7bit(src: &[u8], dst: &mut Vec<u8>)
{
    // Iteration
    // |  Leftover bits
    // |  |         7-bit output
    // |  |         |
    // 0: ........ 00000000 -> yield 7 bits
    // 1: .......1 11111110 -> yield 7 bits
    // 2: ......22 22222211 -> yield 7 bits
    // 3: .....333 33333222 -> yield 7 bits
    // 4: ....4444 44443333 -> yield 7 bits
    // 5: ...55555 55544444 -> yield 7 bits
    // 6: ..666666 66555555 -> yield 7 bits, then
    //    ........ .6666666 -> yield 7 bits again
    // 7: (repeats)

    let mut data = 0u16;    // a shift register where bytes become bits
    let mut bits = 0;       // how many leftover bits from previous iteration

    for v in src {
        // Add 8 input bits.
        data |= (*v as u16) << bits;

        // Yield 7 bits.  Accrue 1 leftover bit for next iteration.
        dst.push((data & 0x7F) as u8);
        data >>= 7;
        bits  += 1;

        // Every 7 iterations, 7 leftover bits have accrued.
        // Consume them to yield another 7-bit output.
        if bits == 7 {
            dst.push((data & 0x7F) as u8);
            data = 0;
            bits = 0;
        }
    }

    // Yield final leftover bits, if any.
    if bits > 0 {
        dst.push((data & 0x7F) as u8);
    }
}

/// Decodes a sequence of 7-bit values into a sequence of bytes.
pub fn decode_7bit(src: &[u8], dst: &mut Vec<u8>)
{
    // Iteration
    // |  Leftover bits
    // |  |        Byte output
    // |  |        |
    // 0: ........ .0000000 (not enough bits for a byte)
    // 1: ..111111 10000000 -> yield byte
    // 2: ...22222 22111111 -> yield byte
    // 3: ....3333 33322222 -> yield byte
    // 4: .....444 44443333 -> yield byte
    // 5: ......55 55555444 -> yield byte
    // 6: .......6 66666655 -> yield byte
    // 7: ........ 77777776 -> yield byte
    // 8: (repeats)

    let mut data = 0u16;    // a shift register where bits become bytes
    let mut bits = 0;       // how many leftover bits from previous iteration

    for v in src {
        // Isolate 7 input bits.
        let v = (*v & 0x7F) as u16;

        if bits == 0 {
            // Initially, and after every 8 iterations, there are no leftover
            // bits from the previous iteration.  With only 7 new bits, there
            // aren't enough to make a byte.  Just let those bits become the
            // leftovers for the next iteration.
            data = v;
            bits = 7;
        } else {
            // For other iterations, there are leftover bits from the previous
            // iteration.  Consider those as least significant, and the 7 new
            // bits as most significant, and yield a byte.  Any unused bits
            // become leftovers for the next iteration to use.
            data |= v << bits;
            dst.push((data & 0xFF) as u8);
            data >>= 8;
            bits  -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_7bit() {
        let data8 = [
            0xF1, 0xE2, 0xD3, 0xC4, 0xB5, 0xA6, 0x97, 0x88, 0x79, 0x6A,
        ];
        let mut data7 = vec![];

        encode_7bit(&data8, &mut data7);

        assert_eq!(data7.len(), 12);
        //                       always 0
        //                       | new bits
        //                       | |    leftover bits
        //                       | |    |
        //                    0b_x_xxxx_xxx
        assert_eq!(data7[ 0], 0b_0_1110001_);
        assert_eq!(data7[ 1], 0b_0_100010_1);
        assert_eq!(data7[ 2], 0b_0_10011_11);
        assert_eq!(data7[ 3], 0b_0_0100_110);
        assert_eq!(data7[ 4], 0b_0_101_1100);
        assert_eq!(data7[ 5], 0b_0_10_10110);
        assert_eq!(data7[ 6], 0b_0_1_101001);
        assert_eq!(data7[ 7], 0b_0__1001011);
        assert_eq!(data7[ 8], 0b_0_0001000_);
        assert_eq!(data7[ 9], 0b_0_111001_1);
        assert_eq!(data7[10], 0b_0_01010_01);
        assert_eq!(data7[11], 0b_0_0000_011);
        //                         |    |
        //                         |    final leftover bits
        //                         0-padding
    }

    #[test]
    fn test_decode_7bit() {
        let data7 = [
        //     don't care
        //     | leftover bits
        //     | |    new bits
        //     | |    |
        //  0b_x_xxxx_xxx
            0b_1_1110001_,
            0b_0_100010_1,
            0b_1_10011_11,
            0b_0_0100_110,
            0b_1_101_1100,
            0b_0_10_10110,
            0b_1_1_101001,
            0b_0__1001011,
            0b_1_0001000_,
            0b_0_111001_1,
            0b_1_01010_01,
            0b_0_1111_011,
        ];
        let mut data8 = vec![];

        decode_7bit(&data7, &mut data8);

        assert_eq!(data8.len(), 10);
        assert_eq!(data8[0], 0xF1);
        assert_eq!(data8[1], 0xE2);
        assert_eq!(data8[2], 0xD3);
        assert_eq!(data8[3], 0xC4);
        assert_eq!(data8[4], 0xB5);
        assert_eq!(data8[5], 0xA6);
        assert_eq!(data8[6], 0x97);
        assert_eq!(data8[7], 0x88);
        assert_eq!(data8[8], 0x79);
        assert_eq!(data8[9], 0x6A);
        // Final leftover 4 bits go unused.
    }
}

