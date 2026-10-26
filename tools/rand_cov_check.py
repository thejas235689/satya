#!/usr/bin/env python3
"""Measure Satya's randomisation and coverage, and check both.

Two things are easy to get wrong in a PSS engine and hard to notice by eye: a
`dist` whose weights are not honoured, and a constraint that is reported
satisfied but not actually enforced. Coverage is the same — a bin that is
declared but never sampled looks exactly like one that is simply rare.

This script runs each example under many seeds and checks the result against
the *model's own intent*, restated here in Python, rather than against the
tool's self-report.

Usage:
    python3 tools/rand_cov_check.py [satya-binary] [--seeds N]

Exit status is 1 if any check fails.
"""

import collections
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAND = os.path.join(HERE, "examples", "randomisation")
COV = os.path.join(HERE, "examples", "coverage")


def run(binary, model, action, seed):
    r = subprocess.run(
        [binary, "solve", model, "--action", action, "--seed", str(seed)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    return r.stdout + r.stderr


def solved(out):
    """The `solved: k=v k=v` map of the first step."""
    for line in out.split("\n"):
        line = line.strip()
        if line.startswith("solved:"):
            fields = {}
            for tok in line[len("solved:"):].split():
                if "=" in tok:
                    k, v = tok.split("=", 1)
                    try:
                        fields[k] = int(v)
                    except ValueError:
                        fields[k] = v
            return fields
    return {}


def constraint_totals(out):
    """(satisfied, total) from the `constraints:` line, or None."""
    m = re.search(r"constraints:\s*(\d+)/(\d+)", out)
    return (int(m.group(1)), int(m.group(2))) if m else None


def coverage(out):
    """{point: (values_observed, hits, (bins_hit, bins_total) or None)}."""
    res = {}
    in_cov = False
    for line in out.split("\n"):
        if line.strip() == "Coverage":
            in_cov = True
            continue
        if not in_cov:
            continue
        if not line.strip():
            continue
        if set(line.strip()) == {"-"}:
            continue  # the rule under the `Coverage` heading
        if line.startswith("  ") and not line.startswith("   "):
            body = line.strip()
            m = re.match(r"(\S+)\s+(\d+) value\(s\) observed, (\d+) hit\(s\)", body)
            if m:
                bins = None
                bm = re.search(r"bins (\d+)/(\d+)", body)
                if bm:
                    bins = (int(bm.group(1)), int(bm.group(2)))
                res[m.group(1)] = (int(m.group(2)), int(m.group(3)), bins)
        elif not line.startswith("  "):
            in_cov = False
    return res


def chi_square(observed, expected_fracs, total):
    """Pearson chi-square against the declared proportions."""
    chi = 0.0
    for value, frac in expected_fracs.items():
        exp = frac * total
        obs = observed.get(value, 0)
        if exp > 0:
            chi += (obs - exp) ** 2 / exp
    return chi


# A generous bound: with 5 categories, chi-square at p=0.001 is about 18.5.
# A solver that ignores the weights lands far above it.
CHI_LIMIT = 25.0


def check_dist(binary, seeds, failures):
    print("\nrandomisation: dist weights")
    print("-" * 62)
    model = os.path.join(RAND, "r1_dist_weights.pss")

    # `constraint dist pick in [0 := 1, 1 := 3, 10..12 :/ 6]`
    # `:/` divides 6 across 10,11,12, so each gets 2. Total weight 10.
    expected = {0: 0.1, 1: 0.3, 10: 0.2, 11: 0.2, 12: 0.2}
    observed = collections.Counter()
    bad = 0
    for s in range(1, seeds + 1):
        v = solved(run(binary, model, "dist_a", s)).get("pick")
        if v is None:
            bad += 1
            continue
        observed[v] += 1
    total = sum(observed.values())
    outside = [v for v in observed if v not in expected]
    chi = chi_square(observed, expected, total)
    print("  pick        %d seeds, %d distinct values" % (total, len(observed)))
    for v in sorted(expected):
        print(
            "    %-3d expected %5.1f%%   observed %5.1f%%  (%d)"
            % (v, expected[v] * 100, 100.0 * observed.get(v, 0) / max(total, 1), observed.get(v, 0))
        )
    print("  chi-square %.2f (limit %.1f)" % (chi, CHI_LIMIT))
    if outside:
        print("  FAIL values outside the dist: %s" % sorted(outside))
        failures.append("dist: values outside the distribution")
    if chi > CHI_LIMIT:
        print("  FAIL the observed distribution does not follow the weights")
        failures.append("dist: weights not honoured (chi-square %.1f)" % chi)
    if bad:
        print("  FAIL %d seeds produced no solution" % bad)
        failures.append("dist: %d unsolved seeds" % bad)


def check_constraints(binary, seeds, failures):
    """Verify each solution against the model's intent, restated here."""
    print("\nrandomisation: constraints hold for every seed")
    print("-" * 62)
    model = os.path.join(RAND, "r2_constraints.pss")
    array_model = os.path.join(RAND, "r3_arrays.pss")
    unique_model = os.path.join(RAND, "r4_unique.pss")

    def constr_a(f):
        errs = []
        if not (1 <= f.get("len", 0) <= 64):
            errs.append("len out of [1..64]")
        if not (0 <= f.get("addr", -1) <= 1023):
            errs.append("addr out of [0..1023]")
        if f.get("addr", 1) % 4 != 0:
            errs.append("addr % 4 != 0")
        kind, burst = f.get("kind"), f.get("burst")
        if kind == 1 and not (2 <= burst <= 8):      # WRITE
            errs.append("WRITE burst not in [2..8]")
        if kind == 0 and burst != 0:                 # READ
            errs.append("READ burst != 0")
        if burst > (4 if f.get("len", 0) > 32 else 2):
            errs.append("if/else burst bound violated")
        return errs

    def ordered_a(f):
        return [] if f.get("lo", 0) < f.get("hi", 0) else ["lo < hi violated"]

    def array_a(f):
        errs = []
        for i in range(8):
            v = f.get("payload[%d]" % i)
            if v is None:
                errs.append("payload[%d] missing" % i)
            elif not (0 <= v <= 15):
                errs.append("payload[%d]=%d out of [0..15]" % (i, v))
        if not (1 <= f.get("n", 0) <= 8):
            errs.append("n out of [1..8]")
        return errs

    def sized_a(f):
        return [] if all(1 <= f.get("ids[%d]" % i, 0) <= 4 for i in range(4)) else ["ids out of [1..4]"]

    def unique_ok_a(f):
        vals = [f.get("ids[%d]" % i) for i in range(4)]
        if any(v is None or not (1 <= v <= 4) for v in vals):
            return ["ids out of [1..4]"]
        return [] if len(set(vals)) == 4 else ["ids are not distinct: %s" % vals]

    def unique_scalars_a(f):
        vals = [f.get("x"), f.get("y"), f.get("z")]
        if any(v is None or not (1 <= v <= 3) for v in vals):
            return ["x/y/z out of [1..3]"]
        return [] if len(set(vals)) == 3 else ["x/y/z not distinct: %s" % vals]

    for action, checker in (
        ("constr_a", constr_a),
        ("ordered_a", ordered_a),
        ("array_a", array_a),
        ("sized_a", sized_a),
        ("unique_ok_a", unique_ok_a),
        ("unique_scalars_a", unique_scalars_a),
    ):
        bad = 0
        sample = None
        if action in ("array_a", "sized_a"):
            mdl = array_model
        elif action in ("unique_ok_a", "unique_scalars_a"):
            mdl = unique_model
        else:
            mdl = model
        for s in range(1, seeds + 1):
            out = run(binary, mdl, action, s)
            f = solved(out)
            errs = checker(f)
            if errs:
                bad += 1
                if sample is None:
                    sample = (s, f, errs)
        status = "ok" if bad == 0 else "FAIL"
        print("  %-12s %3d seeds  %s" % (action, seeds, status))
        if sample:
            s, f, errs = sample
            print("    seed %d: %s -> %s" % (s, f, ", ".join(errs)))
            failures.append("constraints: %s violates its model" % action)


def check_unique_unsatisfiable(binary, failures):
    """Five distinct values from [1..4] cannot exist: the tool must say so."""
    print("\nrandomisation: an unsatisfiable unique is reported, not faked")
    print("-" * 62)
    model = os.path.join(RAND, "r4_unique.pss")
    out = run(binary, model, "unique_impossible_a", 1)
    f = solved(out)
    vals = [f.get("five[%d]" % i) for i in range(5)]
    tot = constraint_totals(out)
    honest = tot is not None and tot[0] < tot[1]
    print("  values %s, constraints %s" % (vals, "%d/%d" % tot if tot else "none"))
    if len(set(v for v in vals if v is not None)) == 5:
        print("  FAIL returned five distinct values from a domain of four")
        failures.append("unique: impossible case produced a fake solution")
    elif not honest:
        print("  FAIL did not report the constraint as unsatisfied")
        failures.append("unique: impossible case reported as satisfied")
    else:
        print("  ok: reported unsatisfied")


def check_coverage(binary, seeds, failures):
    """Cumulative bin coverage, computed independently of the tool's report.

    A single run samples one value, so the tool's per-run `bins 1/3` is not a
    coverage figure. Real coverage accumulates across the whole test, so this
    maps each seed's solved values onto the declared bins and reports the
    cumulative result — and cross-checks that the tool reported the same
    coverage points.
    """
    print("\ncoverage: every declared bin is reachable across seeds")
    print("-" * 62)
    # (file, action, field, {bin: (lo, hi)}, points the tool must report)
    cases = [
        ("c1_bins.pss", "top_a", "len",
         {"short_len": (0, 3), "long_len": (4, 7), "exact_15": (15, 15)},
         ["len_cg.len", "len_cg.kind"]),
        ("c2_cross.pss", "top_a", "len",
         {"small": (0, 3), "large": (4, 7)},
         ["xfer_cg.len", "xfer_cg.kind", "xfer_cg.cross(len,kind)"]),
        ("c3_labelled.pss", "top_a", "len",
         {"small": (0, 3), "large": (4, 7)},
         ["mode_cg.len_bin", "mode_cg.mode_bin", "mode_cg.len_x_mode"]),
    ]
    for fname, action, field, bins, must_report in cases:
        model = os.path.join(COV, fname)
        hit = collections.Counter()
        reported = set()
        sampled = 0
        if action in ("array_a", "sized_a"):
            mdl = array_model
        elif action in ("unique_ok_a", "unique_scalars_a"):
            mdl = unique_model
        else:
            mdl = model
        for s in range(1, seeds + 1):
            out = run(binary, mdl, action, s)
            f = solved(out)
            v = f.get(field)
            if v is not None:
                sampled += 1
                for name, (lo, hi) in bins.items():
                    if lo <= v <= hi:
                        hit[name] += 1
                        break
            for name in coverage(out):
                reported.add(name)
        print("  %-16s %d seed(s), %d sampled" % (fname, seeds, sampled))
        for name in bins:
            c = hit.get(name, 0)
            mark = "" if c else "   <-- never hit"
            print("    bin %-12s %4d hit(s)%s" % (name, c, mark))
            if not c:
                failures.append("coverage: %s bin %s never hit" % (fname, name))
        for name in must_report:
            if name not in reported:
                print("    FAIL the tool never reported %s" % name)
                failures.append("coverage: %s not reported for %s" % (name, fname))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    seeds = 200
    for a in sys.argv[1:]:
        if a.startswith("--seeds"):
            seeds = int(a.split("=", 1)[1]) if "=" in a else seeds
    binary = args[0] if args else "satya"

    failures = []
    print("Satya randomisation and coverage check")
    print("=" * 62)
    check_dist(binary, seeds, failures)
    check_constraints(binary, min(seeds, 60), failures)
    check_unique_unsatisfiable(binary, failures)
    check_coverage(binary, min(seeds, 60), failures)
    print("\n" + "=" * 62)
    if failures:
        print("FAILED (%d):" % len(failures))
        for f in failures:
            print("  - %s" % f)
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
