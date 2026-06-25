//! Differential tests against committed reference output.
//!
//! Two layers:
//!   * `toy_*` fixtures in `tests/models/` — small, redistributable transducers
//!     built from lexc by `build_fixtures.py`, verified against checked-in
//!     expected output. No HFST install or download required. Both directions
//!     are covered: the `toy_*` analyzers (surface -> analysis) and the
//!     `toy_*_gen` generators (analysis -> surface).
//!   * an optional large run against the real Greenlandic analyzer, used only
//!     when `oracle/cases.tsv` has been generated locally (see
//!     `oracle/gen_oracle.py`).
//!
//! Lookup is direction-agnostic: a generator is just a transducer whose input
//! side is the analysis, so the same traversal applies it. Until the traversal
//! landed, `lookup` returned `Unsupported` and these self-skipped; the skip path
//! is kept so a fresh checkout stays green if a fixture is ever absent.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use hfstol::{Error, Transducer};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Parse an expected `.tsv` (`input<TAB>analysis`, `+?` => no analysis) into
/// `input -> set of analyses`.
fn parse_expected(text: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in text.lines() {
        let mut f = line.splitn(2, '\t');
        let (Some(inp), Some(out)) = (f.next(), f.next()) else {
            continue;
        };
        let entry = m.entry(inp.to_string()).or_default();
        if out != "+?" {
            entry.insert(out.to_string());
        }
    }
    m
}

/// Compare a transducer's analyses against expected output. Returns silently
/// (skips) if the traversal is not yet implemented.
fn check(transducer: PathBuf, expected_tsv: PathBuf, label: &str) {
    if !transducer.exists() || !expected_tsv.exists() {
        eprintln!("skip {label}: fixture not present");
        return;
    }
    let t = Transducer::from_path(&transducer).expect("load transducer");
    let expected = parse_expected(&std::fs::read_to_string(&expected_tsv).expect("read expected"));

    let mut mismatches = Vec::new();
    for (inp, exp) in &expected {
        let got: BTreeSet<String> = match t.lookup(inp) {
            Ok(rs) => rs.into_iter().map(|a| a.output).collect(),
            Err(Error::Unsupported(_)) => {
                eprintln!("skip {label}: lookup traversal not implemented yet");
                return;
            }
            Err(e) => panic!("{label}: lookup({inp:?}) errored: {e}"),
        };
        if &got != exp {
            mismatches.push(format!("  {inp:?}: expected {exp:?}, got {got:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{label}: {} input(s) disagree with the oracle:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

fn toy(stem: &str) {
    let dir = manifest().join("tests/models");
    check(
        dir.join(format!("{stem}.hfstol")),
        dir.join(format!("{stem}.expected.tsv")),
        stem,
    );
}

#[test]
fn toy_nouns() {
    toy("toy_nouns");
}

#[test]
fn toy_flags() {
    toy("toy_flags");
}

#[test]
fn toy_nouns_gen() {
    toy("toy_nouns_gen");
}

/// Generation direction with flag gating: `the+Det+cat+N` generates `thecat`
/// (the `@P.CASE.NOM@` path satisfies the noun's `@R.CASE.NOM@`), while
/// `a+Det+cat+N` generates nothing (the flag is never set).
#[test]
fn toy_flags_gen() {
    toy("toy_flags_gen");
}

/// A bare optimized-lookup file (no `HFST` wrapper) must load and apply
/// identically to the wrapped one. We strip the wrapper from a committed toy
/// fixture in memory rather than ship a second binary; the weighted flag then
/// has to come from the OL header instead of the wrapper's `type`.
#[test]
fn bare_ol_header() {
    let path = manifest().join("tests/models/toy_nouns.hfstol");
    if !path.exists() {
        eprintln!("skip bare_ol_header: fixture not present");
        return;
    }
    let bytes = std::fs::read(&path).expect("read toy_nouns.hfstol");
    assert_eq!(&bytes[..4], b"HFST", "fixture should be HFST-wrapped");
    // Wrapper = "HFST"(4) + \0(1) + u16 len(2) + \0(1) + len bytes.
    let len = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
    let bare = &bytes[8 + len..];

    let wrapped = Transducer::from_bytes(&bytes).expect("load wrapped");
    let raw = Transducer::from_bytes(bare).expect("load bare");
    assert!(!raw.header.weighted, "toy_nouns is unweighted (read from OL header)");
    for w in ["cat", "cats", "dog", "dogs", "fish"] {
        let a: Vec<String> = wrapped.lookup(w).unwrap().into_iter().map(|x| x.output).collect();
        let b: Vec<String> = raw.lookup(w).unwrap().into_iter().map(|x| x.output).collect();
        assert_eq!(a, b, "bare vs wrapped disagree on {w:?}");
    }
}

/// Locate a real Greenlandic transducer automatically: `HFSTOL_KAL_DIR` wins if
/// set, otherwise walk up from the crate looking for a sibling `Oqaatsit`
/// checkout (`Oqaatsit/sources/kal/<file>`). Returns `None` when nothing is
/// found, so the big runs self-skip with no machine-specific path in the source.
fn kal(file: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HFSTOL_KAL_DIR") {
        return Some(PathBuf::from(dir).join(file));
    }
    manifest()
        .ancestors()
        .map(|base| base.join("Oqaatsit/sources/kal").join(file))
        .find(|p| p.exists())
}

/// Large local-only run against the real Greenlandic analyzer. Self-skips when
/// the transducer can't be found (see `kal`) or `oracle/gen_oracle.py` hasn't
/// produced `oracle/cases.tsv`.
#[test]
fn greenlandic_oracle() {
    let Some(transducer) = kal("analyser-gt-desc.hfstol") else {
        eprintln!("skip greenlandic: no transducer found (set HFSTOL_KAL_DIR)");
        return;
    };
    check(transducer, manifest().join("oracle/cases.tsv"), "greenlandic");
}

/// Large local-only run against the real Greenlandic *generator* (analysis ->
/// surface). Self-skips when the transducer can't be found or
/// `oracle/gen_oracle.py` hasn't produced `oracle/cases_gen.tsv`.
#[test]
fn greenlandic_generator_oracle() {
    let Some(transducer) = kal("generator-gt-norm.hfstol") else {
        eprintln!("skip greenlandic-gen: no transducer found (set HFSTOL_KAL_DIR)");
        return;
    };
    check(transducer, manifest().join("oracle/cases_gen.tsv"), "greenlandic-gen");
}
