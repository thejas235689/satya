#!/usr/bin/env python3
"""Check Satya against the pssparser error-suite (github.com/psstools/pssparser).

`error-suite/` is a tool-neutral corpus of deliberately-broken *and*
deliberately-valid PSS. Each case file carries a `//!` header stating what the
case is and what a tool should do with it, so the expectation is set outside
this repository:

    //! case:   SYN-BRACES-EXTRA-CLOSE-COMPONENT
    //! class:  syntax.braces
    //! expect: error
    //! lrm:    18.1

Conventions, taken from the suite's own `case.py`:

    NAME.pss        a case; graded on its header's `expect:`
    NAME.ok.pss     the repaired source for NAME.pss; must parse
    _NAME.pss       a companion of a multi-file case; skipped on its own

Usage:
    python3 tools/pssparser_errsuite_check.py <error-suite-dir> [satya-binary]

Exit status is 1 if any case misses its expectation, so it can gate CI.
"""

import os
import re
import subprocess
import sys
from collections import Counter

ACCEPT = ("accept",)


def header(path):
    """The `//! key: value` header of a case file."""
    meta = {}
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if not line.startswith("//!"):
                    break
                m = re.match(r"//!\s*([a-z-]+):\s*(.*)", line)
                if m:
                    meta[m.group(1)] = m.group(2).strip()
    except OSError:
        pass
    return meta


def parses(binary, path):
    r = subprocess.run(
        [binary, "parse", path], capture_output=True, text=True, timeout=120
    )
    return r.returncode == 0


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip())
        return 2
    root = os.path.join(sys.argv[1], "cases")
    if not os.path.isdir(root):
        print("no `cases/` under {}".format(sys.argv[1]))
        return 2
    binary = sys.argv[2] if len(sys.argv) > 2 else "satya"

    cases, controls = [], []
    for dp, _, fns in os.walk(root):
        for f in fns:
            if not f.endswith(".pss") or f.startswith("_"):
                continue
            (controls if f.endswith(".ok.pss") else cases).append(os.path.join(dp, f))

    print("Satya against the pssparser error-suite")
    print("=" * 66)

    tally = Counter()
    wrong = {}
    for p in sorted(cases):
        h = header(p)
        exp = h.get("expect", "").lower()
        if not exp:
            continue
        cat = os.path.relpath(p, root).split(os.sep)[0]
        want = exp in ACCEPT
        got = parses(binary, p)
        key = (cat, exp)
        if got == want:
            tally[(key, "ok")] += 1
        else:
            tally[(key, "wrong")] += 1
            wrong.setdefault(key, []).append(os.path.relpath(p, root))

    total_ok = total = 0
    for cat in ("accept", "syntax", "semantic"):
        for exp in ("accept", "error", "warning"):
            ok = tally[((cat, exp), "ok")]
            bad = tally[((cat, exp), "wrong")]
            if not ok + bad:
                continue
            print(
                "  {:<9} expect={:<8} {:>4} correct {:>4} wrong".format(
                    cat, exp, ok, bad
                )
            )
            total_ok += ok
            total += ok + bad
    print("  " + "-" * 62)
    print(
        "  {:<9} {:<15} {:>4} correct {:>4} wrong   ({:.0f}%)".format(
            "TOTAL", "", total_ok, total - total_ok, 100.0 * total_ok / max(total, 1)
        )
    )

    # A control is the repaired source, so it must parse. A control that still
    # carries the defect (its file content is unchanged from the case) will not,
    # and is reported rather than counted as a Satya failure.
    c_ok = 0
    c_bad = []
    for p in sorted(controls):
        if parses(binary, p):
            c_ok += 1
        else:
            c_bad.append(os.path.relpath(p, root))
    print(
        "  controls (repaired source, must parse): {}/{}".format(c_ok, len(controls))
    )
    for x in c_bad[:5]:
        print("        {}".format(x))

    print("=" * 66)
    if wrong:
        print("\ncases that miss their expectation (first 12):")
        shown = 0
        for key in sorted(wrong):
            for rel in wrong[key][:3]:
                print("   {} expect={:<8} {}".format(key[0], key[1], rel))
                shown += 1
                if shown >= 12:
                    break
            if shown >= 12:
                break
    return 1 if (total - total_ok or c_bad) else 0


if __name__ == "__main__":
    sys.exit(main())
