# hfstol

A from-scratch, pure-Rust applier for the HFST optimized-lookup transducer
format (`.hfstol`). Load a compiled finite-state morphological analyzer or
generator and run words through it, with no runtime dependencies.

This is not a binding to `libhfst`. The binary format reader and the lookup
traversal are reimplemented in Rust from the documented runtime format and
checked analysis-for-analysis against the reference `hfst-optimized-lookup`
program.

## Status

Working analyzer and generator:

- The header parses against real Giella transducers, both with the HFST3
  wrapper and as a bare optimized-lookup header (no wrapper).
- The alphabet and symbol table parse, and flag diacritics are recognised.
- The index and transition tables load into memory.
- The lookup traversal: input tokenisation, the index/transition table walk,
  weights, and flag-diacritic gating. It is verified analysis-for-analysis
  against the reference `hfst-optimized-lookup`, both on the committed toy
  fixtures and on 15,000 real Greenlandic surface forms (57,916 analyses).
- Generation (analysis to surface) needs no extra code: a generator is just a
  transducer whose input side is the analysis, so the same traversal applies it.
  It is checked against the reference too, on committed toy generator fixtures
  (including flag gating in the generation direction) and the real Greenlandic
  generator.
- The crate is `no_std` + `alloc` and builds for `wasm32-unknown-unknown` (see
  below).

The crate only runs transducers that are already compiled. It does not build
them; that is what `lexc`, `twolc`, and `xfst` are for.

## `no_std` / WASM

The library is `no_std` and only needs `alloc`. The default `std` feature adds
`Transducer::from_path`, the `Io` error variant, and the `std::error::Error`
impl; turn it off to drop every OS dependency:

```sh
cargo build --no-default-features                          # core only
cargo build --lib --no-default-features --target wasm32-unknown-unknown
```

Without `std`, load transducers with `Transducer::from_bytes(&[u8])` and apply
them with `lookup`; everything else is identical. The flag-state map uses a
`BTreeMap` rather than a `HashMap` so there is still no runtime dependency (and
no hashing crate) in either mode.

## Performance

The lookup walks the tables directly and keeps the flag-diacritic state in place
across the depth-first search, undoing each edit on backtrack, so a flag
transition allocates nothing. On one 15,000-word run against the real Greenlandic
analyzer (release build, 57,916 analyses), this came out a little faster than the
native `hfst-optimized-lookup` on the same machine, around 7 seconds against 11.
Treat that as one data point: it depends on the transducer and the machine, so
measure your own workload before relying on it.

## Try the loader

```sh
cargo run --bin diff -- path/to/analyser-gt-desc.hfstol --info
```

This prints the header, the symbol counts, and whether the transducer uses flag
diacritics. To run words through it, drop `--info` and pipe input on stdin (one
word per line, UTF-8 without a BOM):

```sh
echo inuk | cargo run --bin diff -- analyser-gt-desc.hfstol
# inuk    inuk+N+Abs+Sg
```

The same binary runs a generator. Point it at the generator transducer and
feed analyses instead of surface forms:

```sh
echo illu+N+Abs+Pl | cargo run --bin diff -- generator-gt-norm.hfstol
# illu+N+Abs+Pl    illut
```

Like the reference, `diff` prints path weights only when given `-w`
(`--show-weights`).

## Verification

Two layers, both compared against the reference `hfst-optimized-lookup`:

- Committed toy fixtures (`tests/models/toy_*.hfstol` and `*.expected.tsv`).
  These are small transducers built from lexc, one with a regular tagset and one
  carrying flag diacritics, checked in next to the reference's expected output.
  Each comes in both directions: the `toy_*` analyzers and the `toy_*_gen`
  generators (the same lexc, not inverted). `cargo test` verifies the applier
  against them with no HFST install and no download. Rebuild them with
  `tests/models/build_fixtures.py`.
- A local large run. `oracle/gen_oracle.py` runs the reference binary over a
  wordlist against the real Greenlandic analyzer to produce `oracle/cases.tsv`,
  then feeds the resulting analyses back through the generator into
  `oracle/cases_gen.tsv`, both of which `cargo test` reproduces.

A fresh checkout is green: the toy fixtures run the real comparison, and the
large runs self-skip unless the Greenlandic transducers can be found (next to
the crate as a sibling `Oqaatsit` checkout, or via `HFSTOL_KAL_DIR`) and
`cases.tsv` / `cases_gen.tsv` have been generated.

### Test results

Run them with:

```sh
cargo test
```

The committed reference output each run reproduces (from
`tests/models/*.expected.tsv`), analysis on the left, generation on the right:

```
toy_nouns (analyze)       toy_nouns_gen (generate)
  cat   cat+N+Sg            cat+N+Sg   cat
  cats  cat+N+Pl            cat+N+Pl   cats
  dog   dog+N+Sg            dog+N+Sg   dog
  dogs  dog+N+Pl            dog+N+Pl   dogs
  fish  +?

toy_flags (analyze)       toy_flags_gen (generate)
  thecat  the+Det+cat+N      the+Det+cat+N  thecat
  acat    a+Det+cat+N        a+Det+cat+N    +?
  the     +?
  a       +?
```

`cargo test` runs these as a real, output-for-output check against the above
(plus `bare_ol_header`, which loads a wrapperless copy of a fixture):

```
running 7 tests
...
test result: ok. 7 passed; 0 failed; 0 ignored
```

## License

Apache-2.0. The reference implementation is GPLv3, so this port is a clean-room
reimplementation from the documented optimized-lookup format and observed bytes,
not a translation of its source.
