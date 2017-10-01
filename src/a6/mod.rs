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

// Position constants
const OPCODE_POS: usize = 4; // Position of opcode
const DATA_POS:   usize = 5; // Start position of data

// Manufacturer/device identifer bytes
static ID: [u8; 4] = [0x00, 0x00, 0x0E, 0x1D];

/// Operation types in an A6 System Exclusive message.
#[repr(u8)]
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Opcode {
    PgmDump           = 0x00,
    PgmDumpReq        = 0x01,
    PgmEditBufDump    = 0x02,
    PgmEditBufDumpReq = 0x03,
    MixDump           = 0x04,
    MixDumpReq        = 0x05,
    MixEditBufDump    = 0x06,
    MixEditBufDumpReq = 0x07,
    GlobalDataDump    = 0x08,
    GlobalDataDumpReq = 0x09,
    PgmBankReq        = 0x0A,
    MkxBankReq        = 0x0B,
    DumpAllReq        = 0x0C,
    ModeSelect        = 0x0D,
    Edit              = 0x0E,
    UpdateOs          = 0x30,
    UpdateBoot        = 0x3F,
}

pub fn recognize_sysex(msg: &[u8]) -> Option<(Opcode, &[u8])> {
    use std::mem::transmute;

    if !msg.starts_with(&ID) || msg.len() <= OPCODE_POS {
        return None
    }

    let opcode = msg[OPCODE_POS];
    if opcode > 0x0E && opcode != 0x30 && opcode != 0x3F {
        return None
    }

    let opcode = unsafe { transmute(opcode) };
    Some((opcode, &msg[DATA_POS..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognize_sysex_ok() {
        let msg = &[0x00, 0x00, 0x0E, 0x1D, 0x30, 0x5A, 0xA5];

        let rec = recognize_sysex(msg);

        assert_eq!(rec, Some((Opcode::UpdateOs, &[0x5A, 0xA5][..])))
    }

    #[test]
    fn recognize_sysex_bad_prefix() {
        let msg = &[0x00, 0xFF, 0x0E, 0x1D, 0x30, 0x5A, 0xA5];

        let rec = recognize_sysex(msg);

        assert_eq!(rec, None);
    }

    #[test]
    fn recognize_sysex_bad_opcode() {
        let msg = &[0x00, 0x00, 0x0E, 0x1D, 0xFF, 0x5A, 0xA5];

        let rec = recognize_sysex(msg);

        assert_eq!(rec, None);
    }

    #[test]
    fn recognize_sysex_underflow() {
        let msg = &[0x00, 0x00, 0x0E];

        let rec = recognize_sysex(msg);

        assert_eq!(rec, None);
    }
}

