//! The symbol table (alphabet) and flag-diacritic operations.
//!
//! Immediately after the header come `number_of_symbols` NUL-terminated UTF-8
//! strings. A symbol is one of:
//!   * an ordinary surface/analysis symbol (`a`, `+N`, `+Sg`, …),
//!   * the special epsilon symbol `@_EPSILON_SYMBOL_@` (index 0), or the
//!     identity/unknown markers, or
//!   * a **flag diacritic** like `@P.CASE.NOM@` / `@R.CASE@` that constrains
//!     which paths are valid. Flag diacritics are erased from the output but
//!     gate the traversal — getting them right is what separates a correct
//!     analyzer from one that massively overgenerates.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::header::Cursor;

/// The kind of a flag-diacritic operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagOp {
    /// `@P.f.v@` — set feature `f` to value `v` (positive).
    P,
    /// `@N.f.v@` — set feature `f` to value `v` (negative).
    N,
    /// `@R.f@` / `@R.f.v@` — require `f` to be set / set to `v`.
    R,
    /// `@D.f@` / `@D.f.v@` — disallow `f` being set / set to `v`.
    D,
    /// `@C.f@` — clear feature `f`.
    C,
    /// `@U.f.v@` — unify: `f` must be unset or already equal to `v`.
    U,
}

/// A parsed flag-diacritic symbol.
#[derive(Debug, Clone)]
pub struct FlagDiacriticOperation {
    pub op: FlagOp,
    pub feature: String,
    /// `None` for the valueless forms (`@R.f@`, `@D.f@`, `@C.f@`).
    pub value: Option<String>,
}

impl FlagDiacriticOperation {
    /// Parse a symbol string as a flag diacritic, or `None` if it is not one.
    fn parse(sym: &str) -> Option<FlagDiacriticOperation> {
        let inner = sym.strip_prefix('@')?.strip_suffix('@')?;
        let mut parts = inner.split('.');
        let op = match parts.next()? {
            "P" => FlagOp::P,
            "N" => FlagOp::N,
            "R" => FlagOp::R,
            "D" => FlagOp::D,
            "C" => FlagOp::C,
            "U" => FlagOp::U,
            _ => return None, // e.g. @_EPSILON_SYMBOL_@, @_UNKNOWN_SYMBOL_@
        };
        let feature = parts.next()?.to_string();
        let value = parts.next().map(|s| s.to_string());
        Some(FlagDiacriticOperation {
            op,
            feature,
            value,
        })
    }
}

/// The epsilon symbol is always symbol number 0 in the optimized-lookup format.
pub(crate) const EPSILON: u16 = 0;

/// The transducer's symbol table.
#[derive(Debug, Clone)]
pub struct Alphabet {
    /// Symbol strings, indexed by `SymbolNumber`.
    pub symbols: Vec<String>,
    /// Parallel to `symbols`: the flag-diacritic operation for each symbol that
    /// is one, else `None`.
    pub flags: Vec<Option<FlagDiacriticOperation>>,
    /// Symbol number of `@_IDENTITY_SYMBOL_@`, if present.
    pub(crate) identity: Option<u16>,
    /// Symbol number of `@_UNKNOWN_SYMBOL_@`, if present.
    pub(crate) unknown: Option<u16>,
}

impl Alphabet {
    /// Read `count` NUL-terminated symbol strings, advancing the cursor to the
    /// first byte of the transition-index table.
    pub(crate) fn parse(c: &mut Cursor, count: u16) -> Result<Alphabet> {
        let mut symbols = Vec::with_capacity(count as usize);
        let mut flags = Vec::with_capacity(count as usize);
        let mut identity = None;
        let mut unknown = None;
        for n in 0..count {
            let s = read_cstr(c)?;
            flags.push(FlagDiacriticOperation::parse(&s));
            match s.as_str() {
                "@_IDENTITY_SYMBOL_@" => identity = Some(n),
                "@_UNKNOWN_SYMBOL_@" => unknown = Some(n),
                _ => {}
            }
            symbols.push(s);
        }
        Ok(Alphabet {
            symbols,
            flags,
            identity,
            unknown,
        })
    }

    /// Whether the alphabet contains any flag diacritics.
    pub fn has_flags(&self) -> bool {
        self.flags.iter().any(Option::is_some)
    }

    /// The flag-diacritic operation for symbol `sym`, if it is one.
    pub(crate) fn flag(&self, sym: u16) -> Option<&FlagDiacriticOperation> {
        self.flags.get(sym as usize).and_then(Option::as_ref)
    }

    /// Whether `sym` is an epsilon or flag-diacritic symbol — i.e. consumes no
    /// input and is erased from the output.
    pub(crate) fn is_epsilon_like(&self, sym: u16) -> bool {
        sym == EPSILON || self.flag(sym).is_some()
    }
}

/// Read a single NUL-terminated UTF-8 string from the cursor.
fn read_cstr(c: &mut Cursor) -> Result<String> {
    let rest = c.remaining();
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::Format("unterminated symbol string".into()))?;
    let s = String::from_utf8_lossy(&rest[..end]).into_owned();
    c.take(end + 1)?; // include the NUL
    Ok(s)
}
