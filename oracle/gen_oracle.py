#!/usr/bin/env python3
"""Generate the differential-test oracle.

Runs the reference `hfst-optimized-lookup` binary over a wordlist and writes the
normalised result (one `input<TAB>output` row per result) for `cargo test` to
reproduce. Two directions are produced:

  * `oracle/cases.tsv` — the analyzer over `oracle/words.txt` (surface ->
    analysis).
  * `oracle/cases_gen.tsv` — the generator over `oracle/words_gen.txt`
    (analysis -> surface). The generator wordlist is derived from the analyses
    the analyzer just produced, so the two runs round-trip. Skipped if the
    generator transducer is absent.

The reference binary and the real Greenlandic transducers both already live in
this portable tree; paths below match that layout and can be overridden via env
vars. All four output files are gitignored (regenerate locally).
"""
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def find_root() -> Path:
    """Locate the portable-tree root that holds the HFST tools by walking up from
    this script, so nothing depends on a hardcoded path. Override with VSCODE_ROOT."""
    env = os.environ.get("VSCODE_ROOT")
    if env:
        return Path(env)
    marker = Path("hfst") / "hfst" / "bin" / "hfst-optimized-lookup.exe"
    for base in (HERE, *HERE.parents):
        if (base / marker).exists():
            return base
    raise SystemExit("could not locate the HFST tools; set VSCODE_ROOT")


ROOT = find_root()

OL_BIN = Path(os.environ.get(
    "HFST_OL_BIN", ROOT / "hfst" / "hfst" / "bin" / "hfst-optimized-lookup.exe"))
KAL = ROOT / "projects" / "Oqaatsit" / "sources" / "kal"
TRANSDUCER = Path(os.environ.get("HFST_TRANSDUCER", KAL / "analyser-gt-desc.hfstol"))
GENERATOR = Path(os.environ.get("HFST_GENERATOR", KAL / "generator-gt-norm.hfstol"))

WORDS = HERE / "words.txt"
OUT = HERE / "cases.tsv"
WORDS_GEN = HERE / "words_gen.txt"
OUT_GEN = HERE / "cases_gen.tsv"

# Tiny default corpus so the script runs out of the box; replace words.txt with
# a real Greenlandic wordlist for a meaningful differential run.
DEFAULT_WORDS = ["inuk", "illu", "nuna", "qimmeq", "angut", "arnaq"]


def load_words() -> list[str]:
    if WORDS.exists():
        return [w.strip() for w in WORDS.read_text(encoding="utf-8").splitlines() if w.strip()]
    return DEFAULT_WORDS


def run_oracle(transducer: Path, words: list[str]) -> list[str]:
    """Run the reference binary over `words`, normalised to `input<TAB>output`
    (the weight column is dropped; equal-weight analyses can reorder versus the
    compiled oracle, so analyses are compared as a set) or `input<TAB>+?`."""
    # Feed as UTF-8 *without* BOM; a leading BOM makes the first word unparseable.
    stdin = ("\n".join(words) + "\n").encode("utf-8")
    proc = subprocess.run(
        [str(OL_BIN), str(transducer)],
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        raise SystemExit(proc.returncode)

    rows = []
    for line in proc.stdout.decode("utf-8").splitlines():
        if not line.strip():
            continue
        fields = line.split("\t")
        inp = fields[0]
        if "+?" in line or len(fields) < 2:
            rows.append(f"{inp}\t+?")
        else:
            rows.append(f"{inp}\t{fields[1]}")
    return rows


def main() -> int:
    if not OL_BIN.exists():
        print(f"reference binary not found: {OL_BIN}", file=sys.stderr)
        return 1
    if not TRANSDUCER.exists():
        print(f"transducer not found: {TRANSDUCER}", file=sys.stderr)
        return 1

    words = load_words()
    rows = run_oracle(TRANSDUCER, words)
    OUT.write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"wrote {len(rows)} rows for {len(words)} words -> {OUT}")

    # Generation direction: feed the analyses back through the generator so the
    # two runs round-trip. Skip cleanly if the generator transducer is absent.
    if not GENERATOR.exists():
        print(f"generator not found, skipping generation oracle: {GENERATOR}")
        return 0
    analyses = sorted({r.split("\t", 1)[1] for r in rows if not r.endswith("\t+?")})
    WORDS_GEN.write_text("\n".join(analyses) + "\n", encoding="utf-8")
    gen_rows = run_oracle(GENERATOR, analyses)
    OUT_GEN.write_text("\n".join(gen_rows) + "\n", encoding="utf-8")
    print(f"wrote {len(gen_rows)} rows for {len(analyses)} analyses -> {OUT_GEN}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
