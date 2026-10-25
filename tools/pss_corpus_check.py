#!/usr/bin/env python3
"""Check Satya against the shared PSS corpus (github.com/psstools/pss-corpus).

The corpus ships `curated/` — 104 files in eight buckets — and a
`manifest.toml` recording, per bucket, whether it is expected to parse. Seven
buckets must parse; `pathological/` must not. That gives a check with an
externally-set expectation rather than one we chose ourselves.

Usage:
    python3 tools/pss_corpus_check.py <path-to-corpus-checkout> [satya-binary]

Exit status is 1 if any bucket misses its expectation, so this can gate CI.
"""

import os
import subprocess
import sys

# The buckets the manifest marks `parses = true`.
MUST_PARSE = [
    "example2",
    "stdlib",
    "peakrdl",
    "language-ref",
    "pss31",
    "lrm31",
    "lexical",
]
MUST_NOT_PARSE = ["pathological"]


def run(binary, path):
    """True when the tool parses `path` without error."""
    r = subprocess.run(
        [binary, "parse", path], capture_output=True, text=True, timeout=120
    )
    return r.returncode == 0, (r.stderr or r.stdout).strip()


def first_line(text):
    return text.split("\n")[0] if text else ""


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip())
        return 2
    corpus = os.path.join(sys.argv[1], "curated")
    if not os.path.isdir(corpus):
        print("no `curated/` under {}".format(sys.argv[1]))
        return 2
    binary = sys.argv[2] if len(sys.argv) > 2 else "satya"

    ok_total = 0
    bad = 0
    print("Satya against the shared PSS corpus")
    print("=" * 62)

    for bucket in MUST_PARSE:
        d = os.path.join(corpus, bucket)
        if not os.path.isdir(d):
            continue
        files = sorted(f for f in os.listdir(d) if f.endswith(".pss"))
        failures = []
        for f in files:
            good, msg = run(binary, os.path.join(d, f))
            if not good:
                failures.append((f, first_line(msg)))
        ok_total += len(files) - len(failures)
        bad += len(failures)
        mark = "ok" if not failures else "FAIL"
        print(
            "  {:<14} {:>3} file(s)  {:>3} parsed  {:>2} failed   [{}]".format(
                bucket, len(files), len(files) - len(failures), len(failures), mark
            )
        )
        for name, msg in failures[:5]:
            print("        {:<40} {}".format(name[:40], msg))

    print("-" * 62)
    print("  buckets that must parse: {} passed, {} failed".format(ok_total, bad))

    wrongly = []
    for bucket in MUST_NOT_PARSE:
        d = os.path.join(corpus, bucket)
        if not os.path.isdir(d):
            continue
        files = sorted(f for f in os.listdir(d) if f.endswith(".pss"))
        for f in files:
            good, _ = run(binary, os.path.join(d, f))
            if good:
                wrongly.append(f)
        print(
            "  {:<14} {:>3} file(s)  {:>3} correctly rejected  {:>2} wrongly accepted".format(
                bucket, len(files), len(files) - len(wrongly), len(wrongly)
            )
        )
        for name in wrongly:
            print("        wrongly accepted: {}".format(name))

    # The executable (compliance) tier: instrumented PSS plus legality models.
    # No manifest entry covers it, but it is all valid PSS and should parse.
    comp = os.path.join(sys.argv[1], "compliance")
    if os.path.isdir(comp):
        files = []
        for dp, _, fns in os.walk(comp):
            files += [os.path.join(dp, f) for f in fns if f.endswith(".pss")]
        ok = 0
        cfails = []
        for f in sorted(files):
            good, msg = run(binary, f)
            if good:
                ok += 1
            else:
                cfails.append((os.path.relpath(f, comp), first_line(msg)))
        print(
            "  {:<14} {:>3} file(s)  {:>3} parsed  {:>2} failed".format(
                "compliance", len(files), ok, len(cfails)
            )
        )
        for name, msg in cfails[:5]:
            print("        {:<40} {}".format(name[:40], msg))
        bad += len(cfails)

    print("=" * 62)
    return 1 if (bad or wrongly) else 0


if __name__ == "__main__":
    sys.exit(main())
