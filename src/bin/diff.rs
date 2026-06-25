//! Differential-test harness binary.
//!
//! Mirrors `hfst-optimized-lookup`: read words from stdin (one per line) and,
//! for each, print one `input<TAB>output` line per analysis followed by a blank
//! line, so the output can be diffed line-for-line against the reference binary
//! over the same wordlist. A word with no analysis prints `input<TAB>input<TAB>+?`.
//! Like the reference, weights are shown only with `-w`/`--show-weights`.
//!
//! Usage:
//!   diff <transducer.hfstol> --info        # print header/alphabet summary
//!   diff <transducer.hfstol> < words.txt
//!   diff <transducer.hfstol> -w < words.txt  # also print path weights

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use hfstol::Transducer;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let info = args.iter().any(|a| a == "--info");
    let show_weights = args.iter().any(|a| a == "-w" || a == "--show-weights");
    // The transducer is the first non-flag argument, so flags may appear in any
    // position (as with the reference binary).
    let path = match args.iter().skip(1).find(|a| !a.starts_with('-')) {
        Some(p) => p,
        None => {
            eprintln!("usage: diff [-w|--show-weights|--info] <transducer.hfstol>");
            return ExitCode::FAILURE;
        }
    };

    let t = match Transducer::from_path(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error loading {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    if info {
        print_info(&t);
        return ExitCode::SUCCESS;
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut warned = false;
    for line in stdin.lock().lines() {
        let word = match line {
            Ok(w) => w,
            Err(e) => {
                eprintln!("stdin error: {e}");
                return ExitCode::FAILURE;
            }
        };
        let word = word.trim_end_matches(['\r', '\n']);
        match t.lookup(word) {
            Ok(results) if results.is_empty() => {
                // No analysis: the reference echoes the input and marks it `+?`.
                let _ = writeln!(out, "{word}\t{word}\t+?");
            }
            Ok(results) => {
                for a in results {
                    if show_weights {
                        let _ = writeln!(out, "{word}\t{}\t{}", a.output, a.weight);
                    } else {
                        let _ = writeln!(out, "{word}\t{}", a.output);
                    }
                }
            }
            Err(e) => {
                if !warned {
                    eprintln!("lookup not available yet: {e}");
                    warned = true;
                }
            }
        }
        // The reference separates each input's block with a blank line.
        let _ = writeln!(out);
    }
    ExitCode::SUCCESS
}

fn print_info(t: &Transducer) {
    let h = &t.header;
    println!("HFST optimized-lookup transducer");
    println!("  version            {}", h.version);
    println!("  weighted           {}", h.weighted);
    println!("  input symbols      {}", h.number_of_input_symbols);
    println!("  total symbols      {}", h.number_of_symbols);
    println!("  index table size   {}", h.size_of_index_table);
    println!("  target table size  {}", h.size_of_target_table);
    println!("  deterministic      {}", h.deterministic);
    println!("  minimized          {}", h.minimized);
    println!("  cyclic             {}", h.cyclic);
    println!(
        "  flag diacritics    {} ({} symbols)",
        t.alphabet.has_flags(),
        t.alphabet.flags.iter().filter(|f| f.is_some()).count()
    );
    let sample: Vec<&str> = t
        .alphabet
        .symbols
        .iter()
        .take(8)
        .map(String::as_str)
        .collect();
    println!("  first symbols      {sample:?}");
}
