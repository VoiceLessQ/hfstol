#![cfg_attr(not(feature = "std"), no_std)]

//! A from-scratch, pure-Rust applier for the **HFST optimized-lookup**
//! transducer format (`.hfstol`), differentially verified against the upstream
//! `hfst-optimized-lookup` binary — the same harness discipline used across the
//! author's other library ports.
//!
//! This is **not** a binding to `libhfst`. The format reader and the lookup
//! traversal are reimplemented in Rust from the documented runtime format
//! (Silfverberg & Lindén, 2009) and the standalone, self-contained reference
//! program `hfst-optimized-lookup.cc`.
//!
//! # Status
//! Working analyzer **and** generator. The header (HFST3-wrapped or bare) and
//! alphabet parse against real Giella transducers, the index/transition tables
//! load into memory, and the lookup traversal — tokenisation, the table walk,
//! weights, and **flag diacritics**, the feature that makes a morphological
//! analyzer correct — is verified output-for-output against the reference
//! `hfst-optimized-lookup` on the toy fixtures and 15,000 real Greenlandic
//! forms in both directions. Lookup is direction-agnostic, so a generator
//! (analysis → surface) runs through the same code.
//!
//! # Scope
//! Applying a *compiled* transducer (analysis/generation) only. Compiling
//! transducers (`lexc`/`twolc`/`xfst`) is out of scope, exactly as the
//! sentencepiece port does inference but not training.
//!
//! # `no_std`
//! The crate is `no_std` + `alloc`. The default `std` feature adds
//! [`Transducer::from_path`], the `Io` error variant, and the
//! `std::error::Error` impl; with `--no-default-features` the core
//! [`Transducer::from_bytes`] / [`Transducer::lookup`] API still works (e.g. on
//! `wasm32-unknown-unknown`).
//!
//! # Example
//! ```no_run
//! use hfstol::Transducer;
//! let t = Transducer::from_path("analyser-gt-desc.hfstol")?;
//! for analysis in t.lookup("inuk")? {
//!     println!("{}\t{}", analysis.output, analysis.weight);
//! }
//! # Ok::<(), hfstol::Error>(())
//! ```

// The crate always allocates (Vec/String/BTreeMap), but never needs the OS
// outside the `std` feature, so it builds as `no_std` + `alloc`.
extern crate alloc;

mod alphabet;
mod error;
mod header;
mod transducer;

pub use alphabet::{Alphabet, FlagDiacriticOperation, FlagOp};
pub use error::{Error, Result};
pub use header::Header;
pub use transducer::{Analysis, Transducer};
