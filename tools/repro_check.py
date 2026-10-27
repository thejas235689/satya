#!/usr/bin/env python3
"""Run the reproducers under tests/repro/ against a satya binary.

Each reproducer states what it expects in a header comment; this checks the
behaviour the reports asked for, so a regression shows up as a failure here.
"""
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPRO = os.path.join(REPO, "tests", "repro")


def run(binary, model, action, seed, limit_mb=400):
    """Run one solve under an address-space cap, so an OOM fails fast."""
    cmd = [binary, "solve", model, "--seed", str(seed)]
    if action:
        cmd += ["--action", action]
    pre = "ulimit -v %d; " % (limit_mb * 1024)
    try:
        p = subprocess.run(
            ["bash", "-c", pre + " ".join("'%s'" % c for c in cmd)],
            capture_output=True, text=True, timeout=60,
        )
    except subprocess.TimeoutExpired:
        return None, "timed out"
    if p.returncode != 0:
        return None, (p.stderr or p.stdout).strip().splitlines()[:1]
    return p.stdout, None


def fields(out):
    m = re.search(r"solved:(.*)", out)
    if not m:
        return {}
    return {k: int(v) for k, v in re.findall(r"([\w.\[\]]+)=(-?\d+)", m.group(1))}


def main():
    binary = sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, "target/release/satya")
    failures = []

    def check(name, ok, detail=""):
        print("  %-40s %s" % (name, "ok" if ok else "FAIL"))
        if not ok:
            print("      %s" % detail)
            failures.append(name)

    print("bug1 - bit[64] literals with bit 63 set")
    print("-" * 62)
    model = os.path.join(REPRO, "bug1_bit63_literal.pss")
    for lit, want in [("0x7FFFFFFFFFFFFFFF", 2**63 - 1),
                      ("0x8000000000000000", -2**63),
                      ("0xFFFFFFFFFFFFFFFE", -2)]:
        src = (
            'component c {\n  action a { rand bit[64] x; constraint x == %s; };\n};\n'
            'component pss_top { c cc; action test { activity { do c::a; }; }; };\n' % lit
        )
        tmp = "/tmp/satya_repro_b1.pss"
        open(tmp, "w").write(src)
        out, err = run(binary, tmp, "test", 1)
        got = fields(out or "").get("x")
        check("%s -> %s" % (lit, want), got == want, "got %r %s" % (got, err or ""))

    print("\nbug2 - enum range constraints")
    print("-" * 62)
    model = os.path.join(REPRO, "bug2_enum_range.pss")
    vals = []
    for seed in range(1, 7):
        out, err = run(binary, model, None, seed)
        f = fields(out or "")
        v = next((x for k, x in f.items() if k.endswith("sel")), None)
        if v is None:
            check("seed %d" % seed, False, str(err))
            continue
        vals.append(v)
    check("all values inside [A1..A3] = [1..3]",
          all(1 <= v <= 3 for v in vals), "got %s" % vals)

    print("\nbug3 - wide operand domains")
    print("-" * 62)
    model = os.path.join(REPRO, "bug3_wide_domains_oom.pss")
    want = {0, 0x180000000, 0x4000000000000000, 0x7FFFFFFFFFFFFFFF}
    for seed in (1, 2, 3):
        out, err = run(binary, model, "c::wide", seed)
        if out is None:
            check("seed %d solves under 400 MB" % seed, False, str(err))
            continue
        f = fields(out)
        ok = all(f.get(k) in want for k in ("a", "b"))
        check("seed %d solves, members only" % seed, ok, str(f))

    print("\nbug4 - UVM backend declares its enums")
    print("-" * 62)
    model = os.path.join(REPRO, "bug4_uvm_enum_names.pss")
    p = subprocess.run([binary, "gen", model, "--backend", "uvm", "--seed", "1"],
                       capture_output=True, text=True, timeout=60)
    sv = p.stdout
    check("typedef enum is emitted", "typedef enum" in sv)
    bare = re.search(r"rand int (\w+);\s*// enum", sv)
    check("no bare `int` for an enum field", bare is None, str(bare.group(0) if bare else ""))

    print("\nbug5 - set membership is randomised")
    print("-" * 62)
    model = os.path.join(REPRO, "bug5_set_membership.pss")
    want = {0x40, 0x7F, 0x4000, 0x7FFF}
    seen = set()
    for seed in range(1, 61):
        out, err = run(binary, model, "c::a", seed)
        f = fields(out or "")
        v = next((x for k, x in f.items() if k == "x"), None)
        if v is None:
            check("seed %d" % seed, False, str(err))
            continue
        if v not in want:
            check("seed %d is a member" % seed, False, "got %d" % v)
        seen.add(v)
    check("all 4 members reachable", seen == want, "saw %s" % sorted(seen))

    print("\n" + "=" * 62)
    if failures:
        print("%d FAILED" % len(failures))
        return 1
    print("all reproducers pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
