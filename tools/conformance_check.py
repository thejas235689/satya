#!/usr/bin/env python3
"""Check Satya against examples/conformance/*.pss.

Each example targets one construct. The checks assert the behaviour the clause
requires, not merely that the file parses.
"""
import subprocess, sys, re, collections, os

BIN = sys.argv[1] if len(sys.argv) > 1 else "satya"
D = "examples/conformance"
FAILS = []

def solve(name, action="a", seed=1):
    r = subprocess.run([BIN, "solve", os.path.join(D, name), "--action", action,
                        "--seed", str(seed)], capture_output=True, text=True)
    return r.stdout + r.stderr

# `satya solve --seeds N` runs seeds 1..=N in one process, separating the runs
# with a `--- seed N ---` marker. Process startup (~22 ms) dominated these
# sweeps, so each one below asks for the whole range in a single invocation and
# splits the marker; a parser of swept output must split rather than assume one
# run per invocation.
SEED_MARK = re.compile(r"^--- seed (\d+) ---$", re.M)
_SWEEPS = {}


def split_seeds(text):
    """{seed: that seed's own output} from one `--seeds` run."""
    out = {}
    marks = list(SEED_MARK.finditer(text))
    for i, m in enumerate(marks):
        end = marks[i + 1].start() if i + 1 < len(marks) else len(text)
        out[int(m.group(1))] = text[m.end() + 1:end]
    return out


def sweep(name, action, n):
    """seed -> output for seeds 1..n, from ONE invocation.

    Falls back to one run per seed when the batch run fails or writes to stderr
    (a diagnostic belongs to one seed, not to the sweep), so the error path is
    exactly what it was.
    """
    key = (name, action, n)
    if key not in _SWEEPS:
        r = subprocess.run([BIN, "solve", os.path.join(D, name), "--action", action,
                            "--seeds", str(n)], capture_output=True, text=True)
        if r.returncode != 0 or r.stderr.strip():
            _SWEEPS[key] = {s: solve(name, action, s) for s in range(1, n + 1)}
        else:
            _SWEEPS[key] = split_seeds(r.stdout)
    return _SWEEPS[key]

def fields(text):
    """All solved field maps in a trace."""
    out = []
    for line in text.splitlines():
        m = re.search(r"solved: (.*)", line)
        if m:
            f = {}
            for tok in m.group(1).split():
                k, _, v = tok.partition("=")
                if v.lstrip("-").isdigit():
                    f[k] = int(v)
            out.append(f)
    return out

def bins(text):
    return re.findall(r"bins (\d+)/(\d+)", text)

def ok(cond, label, detail=""):
    if cond:
        print("  ok    %s" % label)
    else:
        print("  FAIL  %s %s" % (label, detail))
        FAILS.append(label)

# ---- data types ------------------------------------------------------------
print("dt1 scalars: widths honoured")
bad = 0
dt1 = sweep("dt1_scalars.pss", "a", 20)
for s in range(1, 21):
    for f in fields(dt1[s]):
        if not (0 <= f.get("u8", 0) <= 255): bad += 1
        if not (0 <= f.get("u16", 0) <= 65535): bad += 1
        if f.get("b1", 0) not in (0, 1): bad += 1
ok(bad == 0, "u8/u16/b1 stay inside their widths", "(%d violations)" % bad)

print("dt2 aggregates: enum range, array, typedef")
bad = 0
dt2 = sweep("dt2_aggregates.pss", "a", 20)
for s in range(1, 21):
    for f in fields(dt2[s]):
        if f.get("f.kind") not in (0, 1): bad += 1
        if not (0 <= f.get("f.n", 0) <= 4): bad += 1
ok(bad == 0, "f.kind in {RD,WR} and f.n in [0..4]", "(%d violations)" % bad)

print("dt3 strings and templates")
t = solve("dt3_strings_templates.pss")
ok('name = "frame"' in t, "string field holds its value")
ok(re.search(r"narrow\.raw=8\b", t) is not None, "pair_s<_,4> masks to 4 bits (200 -> 8)")
ok(re.search(r"wide\.raw=200\b", t) is not None, "pair_s<_,16> holds 200")

# ---- constraints -----------------------------------------------------------
print("cn1 ranges, open ranges, mixed sets")
bad = collections.Counter()
cn1 = sweep("cn1_ranges_sets.pss", "a", 40)
for s in range(1, 41):
    for f in fields(cn1[s]):
        if not (10 <= f.get("x", -1) <= 20): bad["x"] += 1
        if f.get("y") not in (3, 5, 7, 11): bad["y"] += 1
        z = f.get("z")
        if not (z in (1, 4) or 9 <= z <= 12 or z >= 20): bad["z"] += 1
ok(not bad, "x/y/z stay inside their constraints", str(dict(bad)))

print("cn2 dist weights and implication")
kinds = collections.Counter(); impl = 0
cn2 = sweep("cn2_dist.pss", "a", 120)
for s in range(1, 121):
    for f in fields(cn2[s]):
        k, n = f.get("kind"), f.get("size", 0)
        kinds[k] += 1
        if k == 2 and not (64 <= n <= 127): impl += 1
        if k != 2 and not (0 <= n <= 15): impl += 1
total = sum(kinds.values()) or 1
ratio = {k: round(v * 6.0 / total, 2) for k, v in kinds.items()}
ok(set(kinds) == {0, 1, 2}, "all three dist values reachable", str(dict(kinds)))
ok(abs(ratio.get(0, 0) - 1) < 0.8 and abs(ratio.get(1, 0) - 3) < 1.2,
   "dist weights roughly 1:3:2", str(ratio))
ok(impl == 0, "implication holds on every seed", "(%d violations)" % impl)

print("cn3 foreach and unique")
bad = 0
cn3 = sweep("cn3_foreach_unique.pss", "a", 40)
for s in range(1, 41):
    for f in fields(cn3[s]):
        vals = [f.get("ids[%d]" % i) for i in range(4)]
        if any(v is None or not (1 <= v <= 4) for v in vals): bad += 1
        elif len(set(vals)) != 4: bad += 1
ok(bad == 0, "four distinct values, each in [1..4]", "(%d violations)" % bad)

print("cn4 aggregate and if/else")
bad = collections.Counter()
cn4 = sweep("cn4_aggregate.pss", "a", 40)
for s in range(1, 41):
    for f in fields(cn4[s]):
        parts = [f.get("parts[%d]" % i, 0) for i in range(4)]
        if sum(parts) > 200: bad["sum"] += 1
        m, p0 = f.get("mode"), parts[0]
        if m == 0 and p0 != 0: bad["if0"] += 1
        if m != 0 and p0 <= 0: bad["else"] += 1
ok(not bad, "sum() and if/else both hold", str(dict(bad)))

print("cn5 soft constraint")
bad = 0; lo0 = 0; tot = 0
cn5 = sweep("cn5_soft_inline.pss", "a", 20)
for s in range(1, 21):
    for f in fields(cn5[s]):
        lo, hi = f.get("lo", 0), f.get("hi", 0)
        tot += 1
        if lo == 0: lo0 += 1
        if not (lo < hi): bad += 1
ok(bad == 0, "lo < hi on every seed", "(%d violations)" % bad)
ok(lo0 == tot, "soft lo == 0 honoured when it can be", "%d/%d" % (lo0, tot))

# ---- coverage --------------------------------------------------------------
print("co1 bins reachable")
hit = collections.Counter()
co1 = sweep("co1_bins.pss", "top_a", 60)
for s in range(1, 61):
    t = co1[s]
    for line in t.splitlines():
        if "co1" in line or "g." in line:
            pass
    m = re.findall(r"bins (\d+)/(\d+)", t)
    if m: hit["max"] = max(hit.get("max", 0), int(m[0][0]))
ok(hit.get("max", 0) >= 1, "bins are reported", str(dict(hit)))

print("co2 cross combinations")
seen = set()
co2 = sweep("co2_cross.pss", "top_a", 60)
for s in range(1, 61):
    t = co2[s]
    m = re.search(r"solved: (.*)", t)
    if m:
        f = dict(tok.split("=") for tok in m.group(1).split() if "=" in tok)
        if "size" in f and "dir" in f:
            seen.add((int(f["size"]), int(f["dir"])))
ok(len(seen) == 8, "all eight size x dir combinations reached",
   "(%d of 8: %s)" % (len(seen), sorted(seen)))

print("co3 monitor-sampled covergroup")
hits = 0; sampled = 0
co3 = sweep("co3_monitor.pss", "top_a", 30)
for s in range(1, 31):
    t = co3[s]
    if "monitor seq_m" in t and re.search(r"monitor seq_m\s+\d+ hit", t):
        hits += 1
    if re.search(r"g\.(cp_cfg|cp_data)", t):
        sampled += 1
ok(hits > 0, "the monitor matches and reports a hit", "(%d/30 seeds)" % hits)
ok(sampled > 0, "its covergroup is sampled through the match", "(%d/30 seeds)" % sampled)

print()
if FAILS:
    print("%d check(s) failed: %s" % (len(FAILS), ", ".join(FAILS)))
    sys.exit(1)
print("all conformance checks passed")
