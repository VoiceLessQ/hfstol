//! Parsing of the transducer file header.
//!
//! Two parts, decoded from real Giella transducers and the reference
//! `hfst-optimized-lookup.cc`:
//!
//! 1. The outer **HFST3 wrapper** (present on files produced by modern `hfst`):
//!    `"HFST"\0` + `u16` property-block length + `\0` + a block of
//!    `key\0value\0` pairs. The `type` key is `HFST_OL` (unweighted) or
//!    `HFST_OLW` (weighted).
//! 2. The **optimized-lookup header**: `u16` input-symbol count, `u16` total
//!    symbol count, then 13 × `u32` fields (table sizes, state/transition
//!    counts, and nine boolean property flags stored one-per-`u32`).

use alloc::format;
use alloc::string::String;

use crate::error::{Error, Result};

/// A little-endian byte cursor over the transducer file.
pub(crate) struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Cursor { buf, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or_else(|| Error::Format("unexpected end of file".into()))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn f32(&mut self) -> Result<f32> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A boolean property flag, stored as a little-endian `u32` (0 / 1).
    fn flag(&mut self) -> Result<bool> {
        Ok(self.u32()? != 0)
    }
}

/// Decoded transducer header.
#[derive(Debug, Clone)]
pub struct Header {
    /// `version` value from the HFST3 wrapper, e.g. `"3.3"`.
    pub version: String,
    /// True when the transducer is weighted (`type == HFST_OLW`); transitions
    /// then carry a trailing `f32` weight.
    pub weighted: bool,

    pub number_of_input_symbols: u16,
    pub number_of_symbols: u16,
    pub size_of_index_table: u32,
    pub size_of_target_table: u32,
    pub number_of_states: u32,
    pub number_of_transitions: u32,

    pub deterministic: bool,
    pub input_deterministic: bool,
    pub minimized: bool,
    pub cyclic: bool,
    pub has_epsilon_epsilon_transitions: bool,
    pub has_input_epsilon_transitions: bool,
    pub has_input_epsilon_cycles: bool,
    pub has_unweighted_input_epsilon_cycles: bool,
}

impl Header {
    /// Parse the header from the start of the file, advancing `c` to the first
    /// byte of the alphabet (symbol) table.
    pub(crate) fn parse(c: &mut Cursor) -> Result<Header> {
        let (version, wrapper_weighted) = parse_hfst3_wrapper(c)?;

        let number_of_input_symbols = c.u16()?;
        let number_of_symbols = c.u16()?;
        let size_of_index_table = c.u32()?;
        let size_of_target_table = c.u32()?;
        let number_of_states = c.u32()?;
        let number_of_transitions = c.u32()?;

        // Nine boolean flags follow, each a `u32`. The first records whether the
        // transducer is weighted. A wrapped file's `type` already told us, but
        // we must still consume the flag to keep the alphabet offset aligned; a
        // bare OL file has no wrapper, so this flag is the only source.
        let ol_weighted = c.flag()?;
        let weighted = wrapper_weighted.unwrap_or(ol_weighted);
        let deterministic = c.flag()?;
        let input_deterministic = c.flag()?;
        let minimized = c.flag()?;
        let cyclic = c.flag()?;
        let has_epsilon_epsilon_transitions = c.flag()?;
        let has_input_epsilon_transitions = c.flag()?;
        let has_input_epsilon_cycles = c.flag()?;
        let has_unweighted_input_epsilon_cycles = c.flag()?;
        Ok(Header {
            version,
            weighted,
            number_of_input_symbols,
            number_of_symbols,
            size_of_index_table,
            size_of_target_table,
            number_of_states,
            number_of_transitions,
            deterministic,
            input_deterministic,
            minimized,
            cyclic,
            has_epsilon_epsilon_transitions,
            has_input_epsilon_transitions,
            has_input_epsilon_cycles,
            has_unweighted_input_epsilon_cycles,
        })
    }
}

/// Parse the optional `"HFST"` wrapper. Returns `(version, weighted)`, where
/// `weighted` is `None` for a bare optimized-lookup file (no wrapper) — the
/// caller then reads the weighted flag from the OL header itself.
///
/// Layout observed on `hfst`-produced files:
/// `"HFST"` `\0` `u16 len` `\0` `<len bytes of key\0value\0 ...>`. Files written
/// by other tools may omit the wrapper and begin directly with the OL header;
/// the reference `hfst-optimized-lookup` reads both, so we do too.
fn parse_hfst3_wrapper(c: &mut Cursor) -> Result<(String, Option<bool>)> {
    if c.remaining().len() < 4 || &c.remaining()[..4] != b"HFST" {
        // No wrapper: a bare OL file. Leave the cursor at the OL header.
        return Ok((String::new(), None));
    }
    c.take(4)?; // "HFST"
    let z = c.u8()?; // \0
    if z != 0 {
        return Err(Error::Format("malformed HFST cookie".into()));
    }
    let len = c.u16()? as usize;
    let _pad = c.u8()?; // trailing \0 before the property block
    let block = c.take(len)?;

    let mut version = String::new();
    let mut ttype = String::new();
    let mut it = block.split(|&b| b == 0);
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        let key = String::from_utf8_lossy(k);
        let val = String::from_utf8_lossy(v).into_owned();
        match key.as_ref() {
            "version" => version = val,
            "type" => ttype = val,
            _ => {}
        }
    }

    let weighted = match ttype.as_str() {
        "HFST_OLW" => true,
        "HFST_OL" => false,
        other => {
            return Err(Error::Format(format!(
                "unexpected transducer type {other:?} (want HFST_OL or HFST_OLW)"
            )))
        }
    };
    Ok((version, Some(weighted)))
}
