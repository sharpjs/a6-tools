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

use self::State::*;
use self::SkipReason::*;

const SYSEX_BEGIN: u8 = 0xF7;
const SYSEX_END:   u8 = 0xF0;

// Chain:
// - Read input
// - Ignore SysRT
// - Detect SysEx
// - Decode 7to8
// - Verify header
// - Patch image
// - Verify checksum
// - Write image

/// Identifies MIDI System Exclusive messages for a particular device and
/// invokes a handler for each such message or related event.
///
/// This type implements a push model rather than an Iterator-based pull model.
/// Here, the push model simplifies lifetimes, dependencies, and code paths.
pub struct SysExDetector<H: SysExHandler> {
    // General
    state:      State,      // State after prior function

    // Stream
    start:      usize,      // Position in stream of event in progress
    pos:        usize,      // Position in stream of next unconsumed byte

    // Message
    len:        usize,      // Current message length
    cap:        usize,      // Maximum message length
  //pre:        usize,      // Length of message prefix (SYSEX_BEGIN + id)
    buf:        Box<[u8]>,  // Message buffer

    // Destination
    handler:    H,          // Handler for messages and events
}

/// Trait for types that handle incoming MIDI System Exclusive messages and
/// related events from a `SysExDetector`.
pub trait SysExHandler {
    /// Called when the detector encounters a System Exclusive message.
    fn on_message(&mut self, pos: usize, msg: &[u8]) -> bool;

    /// Called when the detector skips a block of bytes.
    fn on_skip(&mut self, pos: usize, len: usize, reason: SkipReason) -> bool;

    /// Called when the detector has exhausted its input data.
    fn on_end(&mut self, pos: usize) -> bool;
}

// Internal state of `SysExDetector`.
enum State {
    Initial,    // Looking for start of SysEx message
  //SysExId,    // Verifying ID of SysEx message
    SysExData,  // Accumulating data of SysEx message
    SkippingMessage,
}

/// Conditions that cause a `SysExReader` to skip input bytes.
pub enum SkipReason {
    /// The bytes did not contain a System Exclusive message.
    NotSysEx,

    /// A System Exclusive message did not contain the required bytes
    /// identifying the manufacturer or device.
    MismatchedId,

    /// A System Exclusive message exceeded the maximum allowed length.
    Overflow,

    /// A System Exclusive message was not of the minimum requred length.
    Underflow,

    /// A System Exclusive message was interrupted by an unexpected status byte.
    UnexpectedByte,

    /// A System Exclusive message was interrupted by end-of-file.
    UnexpectedEof,
}

impl<H: SysExHandler> SysExDetector<H> {
    /// Creates a `SysExDetector` that will scan a sequence of input bytes and
    /// yield messages that contain at most `cap` subsequent bytes.
    ///
    /// When the created detector encounters messages and other events, it will
    /// yield them by invoking methods on the given `handler`.
    pub fn new(cap: usize, handler: H) -> Self {
        use std::cmp::max;

        // Ensure capacity for SYSEX_BEGIN, id bytes, and SYSEX_END
        //let pre = 1 + id.len();
        //let cap = max(cap, pre + 1);

        // Prepare buffer, pre-populating known content
        let mut buf = vec![0; cap].into_boxed_slice();
        //buf[0] = SYSEX_BEGIN;
        //buf[1..pre].copy_from_slice(id);

        // Construct
        Self { state: Initial, start: 0, pos: 0, len: 0, cap, /*pre,*/ buf, handler }
    }

    /// Consumes a chunk of input bytes.
    ///
    /// Returns `true` if the entire chunk is consumed or `false` if the handler
    /// requested an early exit.
    pub fn consume(&mut self, mut bytes: &[u8]) -> bool {
        // Loop until entire chunk is consumed
        while bytes.len() > 0 {
            // Invoke path for state
            let (ok, len) = match self.state {
                Initial         => self.skip_non_sysex(bytes),
              //SysExId         => self.verify_id(bytes),
                SysExData       => self.accumulate_data(bytes),
                SkippingMessage => panic!(), // self.skip_sysex(bytes),
            };

            // Check for early exit
            if !ok { return false; }

            // Advance buffer to next unconsumed byte
            bytes = &bytes[len..];
        }

        // Fully consumed
        true
    }

    // Initial state: skip bytes until the SysEx begin byte (F0) is found.
    fn skip_non_sysex(&mut self, bytes: &[u8]) -> (bool, usize) {
        let mut cnt = 0;
        let mut pos = self.pos;

        for &byte in bytes {
            if byte == SYSEX_BEGIN {
                let start  = self.start;
                let count  = pos - start;
                let ok     = count == 0 || self.handler.on_skip(start, count, NotSysEx);
                self.state = SysExData;
                self.start = pos;
                self.pos   = pos + 1;   // Begin byte is pre-populated in buf;
                self.len   = 1;         //   consider it consumed.
                return (ok, cnt);
            }
            cnt += 1;
            pos += 1;
        }

        self.pos = pos;
        (true, cnt)
    }

    //fn verify_id(&mut self, bytes: &[u8]) -> (bool, usize) {
    //    use std::cmp::min;
    //    assert!(self.len <= self.pre);

    //    let mut len = self.len;
    //    let     pre = self.pre;

    //    let cnt = bytes.iter()
    //        .zip(&self.buf[len..pre])
    //        .take_while(|&(&b, &x)| b == x)
    //        .count();
    //    len += cnt;

    //    if len == pre {
    //        // matched entire prefix
    //        self.state = SysExData;
    //    }

    //    self.pos += cnt;
    //    self.len  = len;
    //    (true, cnt)
    //}

    fn accumulate_data(&mut self, bytes: &[u8]) -> (bool, usize) { 
        let mut cnt = 0;
        let mut len = self.len;
        let mut buf = &mut self.buf[..];

        for &byte in bytes {
            match byte {
                0x00...0x7F => {
                    // Data byte
                    buf[len] = byte;
                    len += 1;
                },
                0xF0 => {
                    // SysEx begin byte
                    let ok = self.handler.on_skip(self.start, cnt, UnexpectedByte);
                    self.state = SysExData;
                    self.pos  += cnt;
                    self.len   = 1;
                    return (ok, cnt);
                },
                0xF7 => {
                    // SysEx end byte
                    buf[len] = byte;
                    cnt += 1;
                    len += 1;
                    let ok = self.handler.on_message(self.start, &buf[..len]);
                    self.state = Initial;
                    self.start = self.pos + len;
                    self.pos   = self.start;
                    self.len   = 0;
                    return (ok, cnt);
                },
                0xF8...0xFF => {
                    // SysRT message: ignore
                },
                _ /* 0x80...0xEF, 0xF1...0xF6 */ => {
                    // Other status byte
                    let ok = self.handler.on_skip(self.start, cnt, UnexpectedByte);
                    self.state = Initial;
                    self.pos  += cnt;
                    self.len   = 0;
                    return (ok, cnt);
                },
            }
            cnt += 1;
        }

        self.pos += cnt;
        self.len  = len;
        (true, cnt)
    }

    fn skip_sysex(&mut self, bytes: &[u8], reason: SkipReason) -> (bool, usize) {
        // In a SysEx message, but skipping due to mismatched id or because the
        // message is too long.  Possible outcomes:
        // * skip through SysEx end byte [F7];
        // * find invalid byte [80-EF|F1-F6]; or,
        // * run out of bytes to check.
        panic!()
    }

    fn finish(&mut self) {
    }
}

/// Checks if `byte` is a MIDI System Real-Time message.
#[inline(always)]
pub fn is_sysrt(byte: u8) -> bool {
    byte >= 0xF8
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

