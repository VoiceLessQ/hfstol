#!/usr/bin/env python3
"""Build the committed toy transducer fixtures and their expected output.

For each `<name>.lexc` + `words_<name>.txt` pair this builds both directions:

  Analyzer (`<name>.hfstol`, surface -> analysis):
    1. compiles the lexc with `hfst-lexc`,
    2. inverts it (the lexc maps analysis:surface, so inverting gives an analyzer),
    3. converts to unweighted optimized-lookup, and
    4. runs the reference `hfst-optimized-lookup` over `words_<name>.txt`.

  Generator (`<name>_gen.hfstol`, analysis -> surface):
    1. reuses the same compiled lexc *without* inverting,
    2. converts to unweighted optimized-lookup,
    3. derives `words_<name>_gen.txt` from the analyses the analyzer produced
       (so the fixtures round-trip), and
    4. runs the reference binary over it.

Each `<stem>.expected.tsv` is normalised to `input<TAB>output` (or `input<TAB>+?`
when there is no result). The .hfstol, wordlist and .expected.tsv are committed
so `cargo test` verifies the Rust applier against checked-in reference output
with no HFST install required. Rerun this only to rebuild the fixtures (needs
the in-tree HFST tools).
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
BIN = ROOT / "hfst" / "hfst" / "bin"

LEXC = BIN / "hfst-lexc.exe"
INVERT = BIN / "hfst-invert.exe"
FST2FST = BIN / "hfst-fst2fst.exe"
OL = BIN / "hfst-optimized-lookup.exe"

FIXTURES = ["toy_nouns", "toy_flags"]


def run(cmd, **kw):
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kw)
    if p.returncode != 0:
        sys.stderr.write(p.stderr.decode("utf-8", "replace"))
        raise SystemExit(f"command failed: {cmd}")
    return p.stdout


def expected_rows(hfstol: Path, words: list[str]) -> list[str]:
    """Run the reference binary over `words`, normalised to `input<TAB>output`
    (or `input<TAB>+?`)."""
    out = run([str(OL), str(hfstol)], input=("\n".join(words) + "\n").encode("utf-8"))
    rows = []
    for line in out.decode("utf-8").splitlines():
        if not line.strip():
            continue
        fields = line.split("\t")
        inp = fields[0]
        if "+?" in line or len(fields) < 2:
            rows.append(f"{inp}\t+?")
        else:
            rows.append(f"{inp}\t{fields[1]}")
    return rows


def write_expected(stem: str, rows: list[str]) -> None:
    (HERE / f"{stem}.expected.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")


def build(name: str) -> None:
    lexc = HERE / f"{name}.lexc"
    hfst = HERE / f"{name}.hfst"
    inv = HERE / f"{name}.inv.hfst"
    hfstol = HERE / f"{name}.hfstol"
    gen_hfstol = HERE / f"{name}_gen.hfstol"

    # Compile once, then build both directions from it.
    run([str(LEXC), str(lexc), "-o", str(hfst)])
    run([str(INVERT), "-i", str(hfst), "-o", str(inv)])
    run([str(FST2FST), "-O", "-i", str(inv), "-o", str(hfstol)])  # analyzer
    run([str(FST2FST), "-O", "-i", str(hfst), "-o", str(gen_hfstol)])  # generator
    hfst.unlink(missing_ok=True)
    inv.unlink(missing_ok=True)

    # Analyzer: surface words -> analyses.
    words = [w.strip() for w in (HERE / f"words_{name}.txt").read_text(
        encoding="utf-8").splitlines() if w.strip()]
    rows = expected_rows(hfstol, words)
    write_expected(name, rows)
    print(f"{name}: {hfstol.name} + {len(rows)} expected rows")

    # Generator: feed back the analyses the analyzer produced, so the fixture
    # round-trips. De-duplicate, keep a stable order.
    analyses = sorted({r.split("\t", 1)[1] for r in rows if not r.endswith("\t+?")})
    (HERE / f"words_{name}_gen.txt").write_text("\n".join(analyses) + "\n", encoding="utf-8")
    gen_rows = expected_rows(gen_hfstol, analyses)
    write_expected(f"{name}_gen", gen_rows)
    print(f"{name}_gen: {gen_hfstol.name} + {len(gen_rows)} expected rows")


def main() -> int:
    for tool in (LEXC, INVERT, FST2FST, OL):
        if not tool.exists():
            print(f"missing HFST tool: {tool}", file=sys.stderr)
            return 1
    for name in FIXTURES:
        build(name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
