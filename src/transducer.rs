//! The transducer itself: the two ordered tables and the lookup entry point.
//!
//! After the alphabet come two arrays:
//!   * the **transition index table** (`size_of_index_table` entries, 6 bytes
//!     each: `u16` input symbol + `u32` target), and
//!   * the **transition table** (`size_of_target_table` entries, 8 bytes each,
//!     plus a trailing `f32` weight when the transducer is weighted: `u16`
//!     input + `u16` output + `u32` target).
//!
//! Lookup walks the input symbol by symbol, following index entries into the
//! transition table, honouring flag diacritics, and collecting the output
//! symbols of every path that reaches a final state.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Reverse;

#[cfg(feature = "std")]
use std::path::Path;

use crate::alphabet::{Alphabet, FlagDiacriticOperation, FlagOp, EPSILON};
use crate::error::Result;
use crate::header::{Cursor, Header};

/// Sentinel input symbol meaning "no symbol" / empty index slot.
pub(crate) const NO_SYMBOL: u16 = u16::MAX;
/// Sentinel target meaning "no transition".
pub(crate) const NO_TARGET: u32 = u32::MAX;
/// Targets at or above this offset address the transition table; below it they
/// address the index table. (`hfst-optimized-lookup`'s `TRANSITION_TARGET_TABLE_START`.)
pub(crate) const TARGET_TABLE_START: u32 = 1 << 31;

/// One entry of the transition index table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexEntry {
    pub input: u16,
    pub target: u32,
}

/// One entry of the transition table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TransitionEntry {
    pub input: u16,
    pub output: u16,
    pub target: u32,
    pub weight: f32,
}

/// A single analysis (or generation) result.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// The concatenated output symbols, e.g. `"inuk+N+Abs+Sg"`.
    pub output: String,
    /// Path weight (always `0.0` for unweighted transducers).
    pub weight: f32,
}

/// A loaded optimized-lookup transducer.
#[derive(Debug, Clone)]
pub struct Transducer {
    pub header: Header,
    pub alphabet: Alphabet,
    pub(crate) index_table: Vec<IndexEntry>,
    pub(crate) transition_table: Vec<TransitionEntry>,
    /// Input symbols eligible for surface tokenisation, ordered longest-string
    /// first so multichar symbols win. Precomputed at load so `lookup` does not
    /// rebuild and re-sort the list on every call.
    tokenizer_order: Vec<u16>,
}

impl Transducer {
    /// Load a transducer from a `.hfstol` file. Requires the `std` feature.
    #[cfg(feature = "std")]
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Transducer> {
        let bytes = std::fs::read(path)?;
        Transducer::from_bytes(&bytes)
    }

    /// Load a transducer from in-memory bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Transducer> {
        let mut c = Cursor::new(bytes);
        let header = Header::parse(&mut c)?;
        let alphabet = Alphabet::parse(&mut c, header.number_of_symbols)?;

        let mut index_table = Vec::with_capacity(header.size_of_index_table as usize);
        for _ in 0..header.size_of_index_table {
            let input = c.u16()?;
            let target = c.u32()?;
            index_table.push(IndexEntry { input, target });
        }

        let mut transition_table = Vec::with_capacity(header.size_of_target_table as usize);
        for _ in 0..header.size_of_target_table {
            let input = c.u16()?;
            let output = c.u16()?;
            let target = c.u32()?;
            let weight = if header.weighted { c.f32()? } else { 0.0 };
            transition_table.push(TransitionEntry {
                input,
                output,
                target,
                weight,
            });
        }

        // The literal, matchable input symbols (surface side) are the first
        // `number_of_input_symbols` entries, excluding epsilon, flags and the
        // identity/unknown markers. Order them longest-string first so
        // multichar symbols win during longest-match tokenisation.
        let n_in = header.number_of_input_symbols as usize;
        let mut tokenizer_order: Vec<u16> = (0..n_in)
            .map(|i| i as u16)
            .filter(|&i| !alphabet.is_epsilon_like(i))
            .filter(|&i| {
                let s = &alphabet.symbols[i as usize];
                !s.is_empty() && Some(i) != alphabet.identity && Some(i) != alphabet.unknown
            })
            .collect();
        tokenizer_order.sort_by_key(|&i| Reverse(alphabet.symbols[i as usize].len()));

        Ok(Transducer {
            header,
            alphabet,
            index_table,
            transition_table,
            tokenizer_order,
        })
    }

    /// Apply the transducer to `input`, returning every accepted analysis.
    ///
    /// The input is tokenised into symbol numbers by longest match against the
    /// alphabet (unmatched characters become `@_UNKNOWN_SYMBOL_@`), then the
    /// index and transition tables are walked depth-first. Epsilon and flag
    /// transitions consume no input; flag diacritics gate the path and are
    /// erased from the output. Every path that reaches a final state at the end
    /// of the input yields one [`Analysis`].
    pub fn lookup(&self, input: &str) -> Result<Vec<Analysis>> {
        Ok(self.walk(input, &[]).0)
    }

    /// Like [`Transducer::lookup`], but also report a break-localization
    /// frontier: the byte offset of the longest input prefix the transducer
    /// accepts as a COMPLETE analysis (a final state reached mid-input).
    /// 0 means no prefix stands alone. For a rejected word a nonzero frontier
    /// marks a "valid word + broken tail" split, which lets a speller
    /// underline the tail instead of the whole word. ("Longest prefix with
    /// live states" was tried first and is useless for polysynthetic
    /// languages: nearly every prefix has live continuations, so it always
    /// reaches the end of the word.)
    ///
    /// `banned_outputs` prunes every path that would emit one of the given
    /// output symbols. Ban the error-tolerant tags of descriptive analyzers
    /// (`+Err/Orth`, `+Err/Sub`) so that a prefix only accepted as a known
    /// misspelling does not count as valid. Empty slice = no pruning.
    pub fn lookup_with_frontier(
        &self,
        input: &str,
        banned_outputs: &[u16],
    ) -> Result<(Vec<Analysis>, usize)> {
        Ok(self.walk(input, banned_outputs))
    }

    /// Symbol numbers whose name satisfies `pred` (e.g. tags like `+Err/Orth`),
    /// for use as `banned_outputs` in [`Transducer::lookup_with_frontier`].
    pub fn symbols_where(&self, mut pred: impl FnMut(&str) -> bool) -> Vec<u16> {
        self.alphabet
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| pred(s))
            .map(|(i, _)| i as u16)
            .collect()
    }

    fn walk(&self, input: &str, banned_outputs: &[u16]) -> (Vec<Analysis>, usize) {
        let tokens = match self.tokenize(input) {
            Some(t) => t,
            None => return (Vec::new(), 0), // a character with no in-alphabet symbol
        };
        let mut walk = Lookup {
            t: self,
            input: &tokens,
            output: Vec::new(),
            flags: FlagState::default(),
            results: Vec::new(),
            max_final_pos: 0,
            banned_outputs,
        };
        walk.step(0, 0, 0.0);
        // Token index -> byte offset: sum the surface text of consumed tokens.
        let frontier = tokens[..walk.max_final_pos.min(tokens.len())]
            .iter()
            .map(|tok| match &tok.orig {
                Some(text) => text.len(),
                None => self.alphabet.symbols[tok.sym as usize].len(),
            })
            .sum();
        (walk.results, frontier)
    }

    /// Split `input` into transducer symbols. Returns `None` if a character
    /// matches no input symbol and the alphabet has no `@_UNKNOWN_SYMBOL_@` to
    /// stand in for it (the word is then unanalysable).
    fn tokenize(&self, input: &str) -> Option<Vec<Token>> {
        // `tokenizer_order` holds the matchable input symbols, longest string
        // first, precomputed at load (see `from_bytes`).
        let mut tokens = Vec::new();
        let mut rest = input;
        'outer: while !rest.is_empty() {
            for &sym in &self.tokenizer_order {
                let s = self.alphabet.symbols[sym as usize].as_str();
                if rest.starts_with(s) {
                    tokens.push(Token { sym, orig: None });
                    rest = &rest[s.len()..];
                    continue 'outer;
                }
            }
            // No literal matched: consume one character as unknown/identity.
            let ch = rest.chars().next().unwrap();
            let orig = ch.to_string();
            rest = &rest[ch.len_utf8()..];
            match self.alphabet.unknown.or(self.alphabet.identity) {
                Some(sym) => tokens.push(Token { sym, orig: Some(orig) }),
                None => return None,
            }
        }
        Some(tokens)
    }
}

/// One tokenised input symbol, plus the original text for identity/unknown
/// symbols so it can be copied through to the output.
#[derive(Clone)]
struct Token {
    sym: u16,
    orig: Option<String>,
}

/// One output symbol on the current path, with the original text carried along
/// when the symbol is identity/unknown.
#[derive(Clone)]
struct OutSym {
    sym: u16,
    orig: Option<String>,
}

/// The flag-diacritic feature state along the *current* path: each set feature
/// maps to a value and a sign (`@N...@` records a *negative* setting).
///
/// The traversal keeps one `FlagState` and edits it in place as it descends,
/// undoing each edit on the way back out (exactly like the `output` stack), so a
/// flag transition costs no map clone. The keys and values are borrowed `&str`
/// into the transducer's own flag operations (which outlive the lookup), so the
/// flag path allocates nothing at all. (A `BTreeMap`, not a `HashMap`, so the
/// crate stays `no_std` + `alloc` with no hashing dependency; the map is tiny.)
#[derive(Default)]
struct FlagState<'a> {
    features: BTreeMap<&'a str, (&'a str, bool)>,
}

/// How to reverse one applied flag operation when the path backtracks.
enum FlagUndo<'a> {
    /// A gating op (`@R@` / `@D@`, an already-satisfied `@U@`, or a valueless
    /// set): nothing changed.
    Nothing,
    /// `feature` was set or cleared; restore its previous binding, or remove it
    /// again if it had none.
    Restore(&'a str, Option<(&'a str, bool)>),
}

impl<'a> FlagState<'a> {
    /// Apply `op` in place. Returns the undo token if the operation is permitted
    /// on this path, or `None` if it blocks (the path is then pruned).
    fn apply(&mut self, op: &'a FlagDiacriticOperation) -> Option<FlagUndo<'a>> {
        let feat = op.feature.as_str();
        let val = op.value.as_deref();
        match op.op {
            // Positive/negative set: always succeed, record the value.
            FlagOp::P | FlagOp::N => {
                let Some(v) = val else { return Some(FlagUndo::Nothing) };
                let prev = self.features.insert(feat, (v, op.op == FlagOp::P));
                Some(FlagUndo::Restore(feat, prev))
            }
            // Clear: always succeed.
            FlagOp::C => {
                let prev = self.features.remove(feat);
                Some(FlagUndo::Restore(feat, prev))
            }
            // Require: feature must be set (to `v`, positively, if given).
            FlagOp::R => match (self.features.get(feat).copied(), val) {
                (Some((cv, true)), Some(v)) if cv == v => Some(FlagUndo::Nothing),
                (Some(_), None) => Some(FlagUndo::Nothing),
                _ => None,
            },
            // Disallow: feature must not be set (to `v`, if given).
            FlagOp::D => match (self.features.get(feat).copied(), val) {
                (None, _) => Some(FlagUndo::Nothing),
                (Some((cv, true)), Some(v)) if cv == v => None,
                (Some(_), Some(_)) => Some(FlagUndo::Nothing),
                (Some(_), None) => None,
            },
            // Unify: succeed if unset or already compatibly set, then set
            // positively to `v`. The read decides; the mutation happens after.
            FlagOp::U => {
                let v = val?;
                match self.features.get(feat).copied() {
                    Some((cv, true)) if cv == v => return Some(FlagUndo::Nothing),
                    Some((cv, false)) if cv != v => {}
                    None => {}
                    _ => return None,
                }
                let prev = self.features.insert(feat, (v, true));
                Some(FlagUndo::Restore(feat, prev))
            }
        }
    }

    /// Reverse the edit recorded by [`FlagState::apply`].
    fn undo(&mut self, undo: FlagUndo<'a>) {
        match undo {
            FlagUndo::Nothing => {}
            FlagUndo::Restore(feature, Some(prev)) => {
                self.features.insert(feature, prev);
            }
            FlagUndo::Restore(feature, None) => {
                self.features.remove(feature);
            }
        }
    }
}

/// Depth-first traversal state for one `lookup` call.
struct Lookup<'a> {
    t: &'a Transducer,
    input: &'a [Token],
    /// Output symbols accumulated on the current path (a stack).
    output: Vec<OutSym>,
    /// Flag-diacritic state on the current path, edited in place and undone on
    /// backtrack.
    flags: FlagState<'a>,
    results: Vec<Analysis>,
    /// Deepest input position at which some path stood in a final state
    /// (i.e. the longest prefix accepted as a complete analysis).
    max_final_pos: usize,
    /// Output symbols that prune a path (error-tolerant sublexica etc.).
    banned_outputs: &'a [u16],
}

impl Lookup<'_> {
    /// Visit the state addressed by `node` with the input cursor at `pos`.
    fn step(&mut self, node: u32, pos: usize, weight: f32) {
        if pos > self.max_final_pos && self.is_final(node) {
            self.max_final_pos = pos;
        }
        if node >= TARGET_TABLE_START {
            let t = (node - TARGET_TABLE_START) as usize;
            // Epsilon/flag transitions of this state begin right after its
            // finality marker at `t`.
            self.epsilon_transitions(t + 1, pos, weight);
            if pos == self.input.len() {
                if let Some(tr) = self.t.transition_table.get(t) {
                    if tr.input == NO_SYMBOL && tr.output == NO_SYMBOL && tr.target == 1 {
                        self.emit(weight + tr.weight);
                    }
                }
                return;
            }
            let tok = self.input[pos].clone();
            self.transitions(t + 1, &tok, pos, weight);
        } else {
            let i = node as usize;
            // The epsilon slot is the first entry of the index state (symbol 0).
            if let Some(e) = self.t.index_table.get(i + 1) {
                if e.input == EPSILON && e.target >= TARGET_TABLE_START {
                    let start = (e.target - TARGET_TABLE_START) as usize;
                    self.epsilon_transitions(start, pos, weight);
                }
            }
            if pos == self.input.len() {
                if let Some(e) = self.t.index_table.get(i) {
                    if e.input == NO_SYMBOL && e.target != NO_TARGET {
                        let fw = if self.t.header.weighted {
                            f32::from_bits(e.target)
                        } else {
                            0.0
                        };
                        self.emit(weight + fw);
                    }
                }
                return;
            }
            let tok = self.input[pos].clone();
            self.find_index(i + 1, &tok, pos, weight);
        }
    }

    /// Follow the epsilon and flag-diacritic transitions of a transition-table
    /// state, starting at `start`. These consume no input.
    fn epsilon_transitions(&mut self, start: usize, pos: usize, weight: f32) {
        let mut j = start;
        while let Some(tr) = self.t.transition_table.get(j) {
            let (input, output, target, w) = (tr.input, tr.output, tr.target, tr.weight);
            if input == NO_SYMBOL {
                break; // start of the next state / end of this block
            }
            if input == EPSILON {
                if self.banned_outputs.contains(&output) {
                    j += 1;
                    continue;
                }
                self.output.push(self.out_sym(output, None));
                self.step(target, pos, weight + w);
                self.output.pop();
            } else if let Some(op) = self.t.alphabet.flag(input) {
                // Flag symbols are erased; record nothing on the output. Edit the
                // flag state in place and undo it when this branch returns.
                if let Some(undo) = self.flags.apply(op) {
                    self.step(target, pos, weight + w);
                    self.flags.undo(undo);
                }
            } else {
                break; // reached the ordinary-symbol transitions
            }
            j += 1;
        }
    }

    /// Take every transition out of a transition-table state (from `start`) that
    /// consumes the input symbol `tok`.
    fn transitions(&mut self, start: usize, tok: &Token, pos: usize, weight: f32) {
        let mut j = start;
        while let Some(tr) = self.t.transition_table.get(j) {
            if tr.input == NO_SYMBOL {
                break;
            }
            if tr.input == tok.sym {
                let (output, target, w) = (tr.output, tr.target, tr.weight);
                if self.banned_outputs.contains(&output) {
                    j += 1;
                    continue;
                }
                self.output.push(self.out_sym(output, tok.orig.clone()));
                self.step(target, pos + 1, weight + w);
                self.output.pop();
            }
            j += 1;
        }
    }

    /// Take the indexed transition for symbol `tok` out of an index-table state
    /// whose symbol slots begin at `base` (= state index + 1).
    fn find_index(&mut self, base: usize, tok: &Token, pos: usize, weight: f32) {
        let slot = base + tok.sym as usize;
        if let Some(e) = self.t.index_table.get(slot) {
            if e.input == tok.sym && e.target >= TARGET_TABLE_START {
                let start = (e.target - TARGET_TABLE_START) as usize;
                self.transitions(start, tok, pos, weight);
            }
        }
    }

    /// Is `node` a final state? (Finality in this format is unconditional:
    /// flag state gates transitions, not acceptance.)
    fn is_final(&self, node: u32) -> bool {
        if node >= TARGET_TABLE_START {
            let t = (node - TARGET_TABLE_START) as usize;
            matches!(self.t.transition_table.get(t),
                Some(tr) if tr.input == NO_SYMBOL && tr.output == NO_SYMBOL && tr.target == 1)
        } else {
            matches!(self.t.index_table.get(node as usize),
                Some(e) if e.input == NO_SYMBOL && e.target != NO_TARGET)
        }
    }

    /// Build an output symbol, carrying the original text for identity/unknown.
    fn out_sym(&self, sym: u16, orig: Option<String>) -> OutSym {
        let orig = if Some(sym) == self.t.alphabet.identity || Some(sym) == self.t.alphabet.unknown {
            orig
        } else {
            None
        };
        OutSym { sym, orig }
    }

    /// Record the current output path as one analysis. Epsilon and flag symbols
    /// are dropped; identity/unknown symbols print their original text.
    fn emit(&mut self, weight: f32) {
        let mut output = String::new();
        for o in &self.output {
            if self.t.alphabet.is_epsilon_like(o.sym) {
                continue;
            }
            match &o.orig {
                Some(text) => output.push_str(text),
                None => output.push_str(&self.t.alphabet.symbols[o.sym as usize]),
            }
        }
        self.results.push(Analysis { output, weight });
    }
}
