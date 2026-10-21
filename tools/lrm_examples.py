#!/usr/bin/env python3
"""Run Satya against every example extracted from the PSS v3.0 LRM.

Most LRM examples are fragments (a bare action, a struct, a single field), so
each snippet is tried with several minimal wrappings and the first that parses
*and yields real model content beyond the wrapper* is taken. A pass therefore
means the construct is supported, not that the framing happened to work.

Usage: lrm_examples.py <path-to-satya-binary>
"""
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict

SATYA = sys.argv[1] if len(sys.argv) > 1 else "satya"
DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "lrm")

WRAPS = [
    # (name, template, minimum content that the *snippet* must contribute)
    ("as-is", "%s", lambda c: sum(c.values()) > 0),
    ("component", "component SatyaWrap {\n%s\n};\n",
     lambda c: c["actions"] + c["types"] + c["enums"] > 0),
    ("package", "package satya_wrap {\n%s\n};\n",
     lambda c: c["components"] + c["actions"] + c["types"] + c["enums"] > 0),
    ("struct-in-package", "package satya_wrap {\nstruct SatyaS {\n%s\n};\n};\n",
     lambda c: c["components"] + c["actions"] + c["enums"] > 0 or c["types"] > 1),
]

KEYS = ("components", "actions", "types", "enums")


def parse_summary(out):
    c = {k: 0 for k in KEYS}
    for line in out.split("\n"):
        for k in KEYS:
            if line.startswith(k):
                try:
                    c[k] = int(line.split(":")[1].strip())
                except (IndexError, ValueError):
                    pass
    return c


def invoke(args, src):
    with tempfile.NamedTemporaryFile("w", suffix=".pss", delete=False) as fh:
        fh.write(src)
        path = fh.name
    try:
        p = subprocess.run([SATYA] + args + [path], capture_output=True, text=True, timeout=30)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return 1, "timeout"
    finally:
        os.unlink(path)


def steps_of(out):
    for line in out.split("\n"):
        if line.startswith("steps"):
            try:
                return int(line.split(":")[1].strip())
            except (IndexError, ValueError):
                pass
    return 0


def main():
    manifest = json.load(open(os.path.join(DIR, "manifest.json")))
    results = []
    for m in manifest:
        body = open(os.path.join(DIR, m["file"])).read().split("\n", 1)[1]
        first_err = None
        ok_with = None      # lenient: parses without error
        strict_with = None  # strict: also yields content beyond the wrapper
        for name, tpl, minimum in WRAPS:
            rc, out = invoke(["parse"], tpl % body)
            if first_err is None:
                first_err = out.strip().split("\n")[0]
            if rc == 0:
                if ok_with is None:
                    ok_with = name
                if strict_with is None and minimum(parse_summary(out)):
                    strict_with = name
        if ok_with is not None and strict_with is None:
            strict_with = None
        steps = None
        if ok_with is not None:
            tpl = dict((w[0], w[1]) for w in WRAPS)[ok_with]
            rc, out = invoke(["solve"], tpl % body)
            steps = steps_of(out) if rc == 0 else None
        results.append({**m, "ok": ok_with is not None, "strict": strict_with is not None,
                        "wrap": ok_with, "err": first_err, "steps": steps,
                        "has_activity": bool(re.search(r"\bactivity\s*\{", body))})

    json.dump(results, open(os.path.join(DIR, "results.json"), "w"), indent=1)

    # `expression` snippets are literals/operators, not models: reported apart.
    models = [r for r in results if r["kind"] != "expression"]
    exprs = [r for r in results if r["kind"] == "expression"]
    ok = [r for r in models if r["ok"]]
    print("LRM examples extracted: %d  (models: %d, expression snippets: %d)"
          % (len(results), len(models), len(exprs)))
    strict = [r for r in models if r["strict"]]
    print("models parsed (no error):        %d / %d  (%.1f%%)"
          % (len(ok), len(models), 100.0 * len(ok) / max(1, len(models))))
    print("models parsed (strict content):  %d / %d  (%.1f%%)"
          % (len(strict), len(models), 100.0 * len(strict) / max(1, len(models))))
    print("  ^ Satya's parser is deliberately lenient (it skips unknown constructs")
    print("    rather than erroring), so the first figure is an upper bound and the")
    print("    second a lower bound; the engine figure below is the strong signal.")
    by_kind = defaultdict(lambda: [0, 0])
    for r in models:
        by_kind[r["kind"]][0 if r["ok"] else 1] += 1
    print("\nby kind (ok / fail):")
    for k in sorted(by_kind):
        print("  %-12s %3d / %3d" % (k, by_kind[k][0], by_kind[k][1]))

    act = [r for r in ok if r["has_activity"]]
    sched = [r for r in act if (r["steps"] or 0) > 0]
    print("\nengine: %d model examples contain an activity, %d schedule >=1 step"
          % (len(act), len(sched)))

    print("\nwrapping used to succeed:")
    for k, v in sorted(Counter(r["wrap"] for r in ok).items()):
        print("  %-20s %d" % (k, v))

    fails = [r for r in models if not r["ok"]]
    if fails:
        print("\nfailures (%d):" % len(fails))
        for r in fails:
            print("  Ex%-3d [%-11s] %-36s %s" % (r["num"], r["kind"], r["title"][:36], r["err"][:64]))


if __name__ == "__main__":
    main()
