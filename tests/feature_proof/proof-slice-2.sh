#!/usr/bin/env bash
#
# proof-slice-2.sh — assertion-based re-derivation of the slice-2 verdicts
# (PSS constructs 26–50: templates, activity/flow constructs, flow objects)
# for Satya v0.46.0.
#
#   ./proof-slice-2.sh /path/to/satya
#
# Every model is embedded below and written into a mktemp directory; nothing
# else is needed on disk. Each construct is judged by ASSERTING on what the
# engine actually did — trace structure, step counts, group markers, solved
# values, flow-object identity — never on the fact that a model parsed.
#
# Output: one line per construct, "PASS <n> <name>" or
#         "FAIL <n> <name> — <what was observed instead>".
# Exit:   0 all 25 pass, 1 any fail, 2 bad/missing/non-Satya binary.
#
set -u

BIN=${1:-}
if [ -z "$BIN" ]; then
    echo "usage: $0 /path/to/satya" >&2
    exit 2
fi
if [ ! -e "$BIN" ]; then
    echo "error: no such file: $BIN" >&2
    exit 2
fi
if [ ! -f "$BIN" ]; then
    echo "error: not a regular file: $BIN" >&2
    exit 2
fi
if [ ! -x "$BIN" ]; then
    echo "error: not executable: $BIN" >&2
    exit 2
fi
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")

HELP=$("$BIN" help 2>&1)
case "$HELP" in
    Satya*) : ;;
    *) echo "error: not a Satya binary: $BIN" >&2; exit 2 ;;
esac

TMP=$(mktemp -d "${TMPDIR:-/tmp}/satya-slice2.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT INT TERM

FAILS=0
REASON=""
SEEDS=$(seq 1 40)

# A failed assertion aborts the construct's check immediately (each check runs in
# a subshell, so `exit` here ends that check and nothing else).
bad() { printf '%s' "$*" > "$TMP/reason"; exit 1; }

solve() { "$BIN" solve "$1" --seed "${2:-1}" 2>&1; }

# ---------------------------------------------------------------- trace access
# Header field, e.g. hdr "$t" "steps" -> 3
hdr() {
    printf '%s\n' "$1" | awk -v k="$2" '
        { line=$0; sub(/[ \t]+$/,"",line)
          n=index(line, ":")
          if (n>0) { key=substr(line,1,n-1); sub(/[ \t]+$/,"",key)
                     if (key==k) { v=substr(line,n+1); sub(/^[ \t]+/,"",v); print v; exit } } }'
}

# Normalised step records: idx|action|component|mode|group|flags
steps_norm() {
    awk '
        /^[ \t]*[0-9]+\. / {
            line=$0; sub(/^[ \t]+/,"",line)
            p=index(line,". "); idx=substr(line,1,p-1); rest=substr(line,p+2)
            at=index(rest," @ "); act=substr(rest,1,at-1); sub(/[ \t]+$/,"",act)
            rest=substr(rest,at+3)
            br=index(rest,"[")
            if (br==0) next
            comp=substr(rest,1,br-1); sub(/[ \t]+$/,"",comp)
            tail=substr(rest,br); cl=index(tail,"]")
            brack=substr(tail,2,cl-2); after=substr(tail,cl+1)
            mode=brack; grp=""
            if (brack ~ / g[0-9]+$/) { n=split(brack,parts," "); grp=parts[n]; sub(" " grp "$","",mode) }
            flags=""; if (after ~ /UNRESOLVED/) flags="UNRESOLVED"
            printf "%s|%s|%s|%s|%s|%s\n", idx, act, comp, mode, grp, flags
        }'
}

nsteps()      { printf '%s\n' "$1" | steps_norm | wc -l | tr -d ' '; }
step_field()  { printf '%s\n' "$1" | steps_norm | awk -F'|' -v i="$2" -v f="$3" '
                  $1==i { if (f=="action") print $2; else if (f=="comp") print $3;
                          else if (f=="mode") print $4; else if (f=="group") print $5;
                          else print $6; exit }'; }

# Solved field value of a step: fval "$t" 0 "narrow.f"
fval() {
    printf '%s\n' "$1" | awk -v i="$2" -v k="$3" '
        /^[ \t]*[0-9]+\. / { line=$0; sub(/^[ \t]+/,"",line); p=index(line,". "); cur=substr(line,1,p-1); next }
        /^[ \t]+solved: / { s=$0; sub(/^[ \t]+solved: /,"",s); n=split(s,a," ")
            for (j=1;j<=n;j++) { e=index(a[j],"=")
                if (e>0) { nm=substr(a[j],1,e-1); val=substr(a[j],e+1)
                           if (cur==i && nm==k) { print val; exit } } } }'
}

# Flow-object id bound to a step's input/output field: fobj "$t" 0 output out
fobj() {
    printf '%s\n' "$1" | awk -v i="$2" -v d="$3" -v f="$4" '
        /^[ \t]*[0-9]+\. / { line=$0; sub(/^[ \t]+/,"",line); p=index(line,". "); cur=substr(line,1,p-1); next }
        /^[ \t]+(input|output) / { s=$0; sub(/^[ \t]+/,"",s); n=split(s,w," ")
            if (cur==i && w[1]==d && w[2]==f) {
                if (match(s, /obj #[0-9]+/)) print substr(s,RSTART+5,RLENGTH-5); else print "-"; exit } }'
}

# Resource instance number claimed by a step: claim "$t" 0
claim() {
    printf '%s\n' "$1" | awk -v i="$2" '
        /^[ \t]*[0-9]+\. / { line=$0; sub(/^[ \t]+/,"",line); p=index(line,". "); cur=substr(line,1,p-1); next }
        /^[ \t]+(lock|share) / { s=$0; sub(/^[ \t]+/,"",s)
            if (cur==i) { if (match(s, /instance [0-9]+/)) print substr(s,RSTART+9,RLENGTH-9); else print "-"; exit } }'
}

warn_has() { printf '%s\n' "$1" | sed -n '/^Warnings/,$p' | grep -q -- "$2"; }

# ================================================================= 26–50 models
cat > "$TMP/c26.pss" <<'EOF'
struct s26 <int W = 8> { rand bit[W] f; };
component pss_top {
   action test {
      s26<4>  narrow;
      s26<16> wide;
      exec pre_solve { narrow.f = 200; wide.f = 200; }
      rand int v;
   };
};
EOF

cat > "$TMP/c27.pss" <<'EOF'
struct pair_s <type T> { T value; };
component t27_c {
   action test {
      pair_s<bit[4]>  narrow;
      pair_s<bit[16]> wide;
      exec pre_solve { narrow.value = 200; wide.value = 200; }
      rand int v;
   };
};
EOF

cat > "$TMP/c28.pss" <<'EOF'
struct pair_s <type T, int W = 8> {
   T      value;
   bit[W] raw;
};
component t28_c {
   action test {
      pair_s<bit[16], 4>  narrow;
      pair_s<bit[8], 16>  wide;
      exec pre_solve { narrow.raw = 200; wide.raw = 200; }
      rand int v;
   };
};
EOF

cat > "$TMP/c29.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test { activity { do A; do B; do C; }; };
};
EOF

cat > "$TMP/c30a.pss" <<'EOF'
component leaf_c { action leaf_a { rand int f; } };
component pss_top {
   leaf_c sub;
   action A { rand int a; }
   action test {
      activity {
         do A;
         do leaf_c::leaf_a;
         do A with { a == 3; };
      };
   };
};
EOF

cat > "$TMP/c30b.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action test { A h1; activity { h1; }; };
};
EOF

cat > "$TMP/c30c.pss" <<'EOF'
component sub_c { action s { rand int g; } };
component pss_top {
   sub_c sub;
   action test { activity { do sub.s; }; };
};
EOF

cat > "$TMP/c31.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         seq1: sequence { do A; do B; };
         do C;
      };
   };
};
EOF

cat > "$TMP/c32.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         par1: parallel { do A; do B; };
         do C;
      };
   };
};
EOF

cat > "$TMP/c33.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         sch1: schedule { do A; do B; };
         do C;
      };
   };
};
EOF

cat > "$TMP/c34_branch.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action D { rand int d; }
   action test {
      activity {
         L1: parallel join_branch(L2) {
            L2: do A;
            L3: do B;
         };
         L4: do C;
         L5: do D;
      };
   };
};
EOF

cat > "$TMP/c34_first.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         L1: parallel join_first(1) { L2: do A; L3: do B; };
         L4: do C;
      };
   };
};
EOF

cat > "$TMP/c34_none.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         L1: parallel join_none { L2: do A; L3: do B; };
         L4: do C;
      };
   };
};
EOF

cat > "$TMP/c34_select.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         L1: parallel join_select(1) { L2: do A; L3: do B; };
         L4: do C;
      };
   };
};
EOF

cat > "$TMP/c35.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity {
         parallel {
            at1: atomic { do A; do B; };
            do C;
         };
      };
   };
};
EOF

cat > "$TMP/c36.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action test { activity { repeat (3) { do A; }; }; };
};
EOF

cat > "$TMP/c37_false.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action test { activity { repeat { do A; } while (0); }; };
};
EOF

cat > "$TMP/c37_true.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action test {
      rand int k in [0..0];
      activity { repeat { do A; } while (k < 2); };
   };
};
EOF

cat > "$TMP/c38.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action test {
      int ids[4];
      activity { foreach (ids[i]) { do A; }; };
   };
};
EOF

cat > "$TMP/c39.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      activity { select { do A; do B; do C; }; };
   };
};
EOF

cat > "$TMP/c40.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action test {
      rand int sel in [0..1];
      activity { if (sel == 0) { do A; } else { do B; }; };
   };
};
EOF

cat > "$TMP/c41.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      rand int v in [0..2];
      activity {
         match (v) {
            0: do A;
            1: do B;
            default: do C;
         };
      };
   };
};
EOF

cat > "$TMP/c42.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action test { activity { replicate (3) { do A; do B; }; }; };
};
EOF

cat > "$TMP/c43.pss" <<'EOF'
component pss_top {
   action A { rand int a; }
   action B { rand int b; }
   action C { rand int c; }
   action test {
      symbol two { do A; do B; }
      activity { two; do C; };
   };
};
EOF

cat > "$TMP/c44_on.pss" <<'EOF'
buffer data_b { rand int size; };
component pss_top {
   pool [4] data_b b_pool;
   bind b_pool *;
   action P { output data_b out; };
   action C { input data_b inp; };
   action test {
      P p1;
      P p2;
      C c;
      activity {
         do P;
         do P;
         do C;
         bind p2.out c.inp;
      };
   };
};
EOF

sed 's/^ *bind p2\.out c\.inp;$//' "$TMP/c44_on.pss" > "$TMP/c44_off.pss"

cat > "$TMP/c45.pss" <<'EOF'
buffer data_b { rand int size; };
component pss_top {
   pool [4] data_b b_pool;
   bind b_pool *;
   action P { output data_b out; };
   action C { input data_b inp; };
   action test { activity { do P; do C; }; };
};
EOF

cat > "$TMP/c46.pss" <<'EOF'
stream data_s { rand int size; };
component pss_top {
   pool [4] data_s s_pool;
   bind s_pool *;
   action P { output data_s out; };
   action C { input data_s inp; };
   action test { activity { do P; do C; do C; }; };
};
EOF

cat > "$TMP/c47.pss" <<'EOF'
state cfg_s { rand int mode; };
component pss_top {
   pool cfg_s cfg_pool;
   bind cfg_pool *;
   action W { output cfg_s nxt; constraint nxt.mode == 5; };
   action R { input cfg_s prv; };
   action test { activity { do W; do R; }; };
};
EOF

cat > "$TMP/c48_order.pss" <<'EOF'
buffer data_b { rand int size; };
component pss_top {
   pool [4] data_b b_pool;
   bind b_pool *;
   action P { output data_b out; };
   action C { input data_b inp; };
   action test { activity { do C; do P; }; };
};
EOF

cat > "$TMP/c48_value.pss" <<'EOF'
buffer data_b { rand int size; };
component pss_top {
   pool [4] data_b b_pool;
   bind b_pool *;
   action P { output data_b out; constraint out.size == 7; };
   action C { input data_b inp; };
   action test { activity { do P; do C; }; };
};
EOF

cat > "$TMP/c49.pss" <<'EOF'
resource core_r { rand int id; };
component pss_top {
   pool [4] core_r core_pool;
   bind core_pool *;
   action A { lock core_r c; };
   action test { activity { parallel { do A; do A; }; }; };
};
EOF

cat > "$TMP/c50.pss" <<'EOF'
resource core_r { rand int id; };
component pss_top {
   pool [4] core_r core_pool;
   bind core_pool *;
   action L { lock core_r c; };
   action S { share core_r c; };
   action test {
      activity {
         parallel { do L; do L; };
      };
   };
};
EOF

# ================================================================== 26–50 checks
c26() {
    local t; t=$(solve "$TMP/c26.pss")
    [ "$(nsteps "$t")" = 1 ] || bad "expected 1 step, saw $(nsteps "$t")"
    local n w; n=$(fval "$t" 0 "narrow.f"); w=$(fval "$t" 0 "wide.f")
    [ "$n" = 8 ]   || bad "value parameter W=4 did not reach bit[W]: 200 in a bit[4] field solved as '$n', expected 8"
    [ "$w" = 200 ] || bad "value parameter W=16 did not reach bit[W]: 200 in a bit[16] field solved as '$w', expected 200"
    return 0
}

c27() {
    local t; t=$(solve "$TMP/c27.pss")
    local n w; n=$(fval "$t" 0 "narrow.value"); w=$(fval "$t" 0 "wide.value")
    [ "$n" = 8 ]   || bad "type parameter did not substitute the field type: 200 in pair_s<bit[4]>.value solved as '$n', expected 8"
    [ "$w" = 200 ] || bad "type parameter did not substitute the field type: 200 in pair_s<bit[16]>.value solved as '$w', expected 200"
    return 0
}

c28() {
    local t; t=$(solve "$TMP/c28.pss")
    local n w; n=$(fval "$t" 0 "narrow.raw"); w=$(fval "$t" 0 "wide.raw")
    [ "$n" = 8 ]   || bad "pair_s<bit[16],4>.raw is not a 4-bit field: 200 solved as '$n', expected 8"
    [ "$w" = 200 ] || bad "pair_s<bit[8],16>.raw is not a 16-bit field: 200 solved as '$w', expected 200"
    return 0
}

c29() {
    local t; t=$(solve "$TMP/c29.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "activity of 3 statements produced $(nsteps "$t") step(s), expected 3"
    [ "$(step_field "$t" 0 action)/$(step_field "$t" 1 action)/$(step_field "$t" 2 action)" = "A/B/C" ] \
        || bad "activity statements out of order: saw $(step_field "$t" 0 action), $(step_field "$t" 1 action), $(step_field "$t" 2 action), expected A, B, C"
    [ -z "$(step_field "$t" 0 group)" ] || bad "a plain sequential activity step carries a parallel group '$(step_field "$t" 0 group)'"
    return 0
}

c30() {
    local t; t=$(solve "$TMP/c30a.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "the three traversal forms produced $(nsteps "$t") step(s), expected 3"
    local i
    for i in 0 1 2; do
        [ -z "$(step_field "$t" $i flags)" ] || bad "traversal form at step $i is $(step_field "$t" $i flags) ('$(step_field "$t" $i action)')"
    done
    [ "$(step_field "$t" 1 comp)" = "pss_top.sub" ] || bad "type-qualified traversal landed on component '$(step_field "$t" 1 comp)', expected pss_top.sub"
    [ "$(fval "$t" 2 a)" = 3 ] || bad "traversal with an inline constraint solved a=$(fval "$t" 2 a), expected 3"

    t=$(solve "$TMP/c30b.pss")
    [ -z "$(step_field "$t" 0 flags)" ] || bad "action-handle traversal \`h1;\` is [UNRESOLVED] (observed action '$(step_field "$t" 0 action)', $(hdr "$t" "concrete steps") concrete step(s)) instead of traversing the action the handle names"
    t=$(solve "$TMP/c30c.pss")
    [ -z "$(step_field "$t" 0 flags)" ] || bad "instance-qualified path \`do sub.s;\` is [UNRESOLVED] (observed action '$(step_field "$t" 0 action)') — the path was truncated to its first segment"
    [ "$(step_field "$t" 0 action)" = "s" ] || bad "instance-qualified path \`do sub.s;\` resolved to action '$(step_field "$t" 0 action)', expected 's'"
    return 0
}

c31() {
    local t; t=$(solve "$TMP/c31.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "sequence block plus trailing statement produced $(nsteps "$t") step(s), expected 3"
    [ "$(step_field "$t" 0 action)/$(step_field "$t" 1 action)/$(step_field "$t" 2 action)" = "A/B/C" ] \
        || bad "sequence steps not in written order: saw $(step_field "$t" 0 action), $(step_field "$t" 1 action), $(step_field "$t" 2 action)"
    local g; g=$(step_field "$t" 0 group)
    [ -z "$g" ] || bad "sequence step carries a concurrent group marker '$g'; sequence must be sequential"
    [ "$(step_field "$t" 1 group)" = "$g" ] || bad "the two sequence steps are in different groups"
    return 0
}

c32() {
    local t; t=$(solve "$TMP/c32.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "parallel block plus trailing statement produced $(nsteps "$t") step(s), expected 3"
    local g0 g1 g2
    g0=$(step_field "$t" 0 group); g1=$(step_field "$t" 1 group); g2=$(step_field "$t" 2 group)
    [ -n "$g0" ] || bad "parallel branch A carries no group id"
    [ "$g0" = "$g1" ] || bad "parallel branches are in different groups ($g0 vs $g1); they must share one group id"
    [ "$(step_field "$t" 0 mode)" = parallel ] || bad "parallel branch mode is '$(step_field "$t" 0 mode)', expected 'parallel'"
    [ -z "$g2" ] || bad "the statement after the parallel block carries group '$g2' — it is inside the concurrent group, not after it"
    return 0
}

c33() {
    local t; t=$(solve "$TMP/c33.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "schedule block plus trailing statement produced $(nsteps "$t") step(s), expected 3"
    local m0 m1 g0 g1 g2
    m0=$(step_field "$t" 0 mode); m1=$(step_field "$t" 1 mode)
    g0=$(step_field "$t" 0 group); g1=$(step_field "$t" 1 group); g2=$(step_field "$t" 2 group)
    [ "$m0" = schedule ] || bad "schedule branch mode is '$m0', expected 'schedule'"
    [ "$m1" = schedule ] || bad "second schedule branch mode is '$m1', expected 'schedule'"
    [ -n "$g0" ] && [ "$g0" = "$g1" ] || bad "schedule branches do not share one group id ($g0 vs $g1)"
    [ -z "$g2" ] || bad "the statement after the schedule block carries group '$g2' — it must run after the block, not inside it"
    return 0
}

c34() {
    local t tn tf ts
    tn=$(solve "$TMP/c34_none.pss"); tf=$(solve "$TMP/c34_first.pss")
    ts=$(solve "$TMP/c34_select.pss"); t=$(solve "$TMP/c34_branch.pss")

    [ "$(nsteps "$t")" = 4 ] || bad "join_branch model produced $(nsteps "$t") step(s), expected 4"
    [ -z "$(step_field "$t" 2 group)" ] \
        || bad "join_branch(L2): the statement after the block (L4: C) carries group '$(step_field "$t" 2 group)' — it is inside the concurrent group, and joins nothing; L5: D is the only step outside"

    [ -z "$(step_field "$tf" 1 group)" ] \
        || bad "join_first(1): the statement after the block (L4: C) carries group '$(step_field "$tf" 1 group)' — it is inside the group rather than gated by one completed branch"
    [ -z "$(step_field "$tn" 1 group)" ] \
        || bad "join_none: the statement after the block (L4: C) carries group '$(step_field "$tn" 1 group)'"
    [ -z "$(step_field "$ts" 1 group)" ] \
        || bad "join_select(1): the statement after the block (L4: C) carries group '$(step_field "$ts" 1 group)' — no branch was selected to gate it"

    [ "$tn" = "$tf" ] \
        || bad "join_none and join_first(1) produce different traces; a run-time dependency and no dependency cannot be the same"
    return 0
}

c35() {
    local t; t=$(solve "$TMP/c35.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "atomic block inside a parallel block produced $(nsteps "$t") step(s), expected 3"
    local m0 m1 m2 g0 g1 g2
    m0=$(step_field "$t" 0 mode); m1=$(step_field "$t" 1 mode); m2=$(step_field "$t" 2 mode)
    g0=$(step_field "$t" 0 group); g1=$(step_field "$t" 1 group); g2=$(step_field "$t" 2 group)
    [ "$m0" = atomic ] || bad "step inside the atomic block has mode '$m0', expected 'atomic'"
    [ "$m0" = "$m1" ] && [ "$g0" = "$g1" ] \
        || bad "the two atomic steps are not one cluster ($m0/$g0 vs $m1/$g1)"
    [ "$m2" != "$m0" ] \
        || bad "the step outside the atomic block has the same mode '$m2' as the atomic steps — the atomic block is not distinguished from its sibling"
    return 0
}

c36() {
    local t; t=$(solve "$TMP/c36.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "repeat(3) produced $(nsteps "$t") step(s), expected 3"
    local i
    for i in 0 1 2; do
        [ "$(step_field "$t" $i action)" = A ] || bad "repeat(3) step $i is '$(step_field "$t" $i action)', expected A"
    done
    return 0
}

c37() {
    local t; t=$(solve "$TMP/c37_false.pss")
    [ "$(nsteps "$t")" = 1 ] || bad "repeat{...}while(0) produced $(nsteps "$t") step(s), expected 1 (body entered once)"
    t=$(solve "$TMP/c37_true.pss")
    local n; n=$(nsteps "$t")
    [ "$n" -gt 1 ] || bad "repeat { do A; } while (k < 2) with k pinned to 0 produced $n step(s) — the condition is never re-evaluated, so the body is traversed once whatever it says"
    return 0
}

c38() {
    local t; t=$(solve "$TMP/c38.pss")
    [ "$(nsteps "$t")" = 4 ] || bad "foreach over int[4] produced $(nsteps "$t") step(s), expected 4"
    local i
    for i in 0 1 2 3; do
        [ "$(step_field "$t" $i action)" = A ] || bad "foreach iteration $i traversed '$(step_field "$t" $i action)', expected A"
    done
    return 0
}

c39() {
    local s t n chosen="" first=""
    for s in $SEEDS; do
        t=$(solve "$TMP/c39.pss" "$s")
        n=$(nsteps "$t")
        [ "$n" = 1 ] || bad "select over 3 branches chose $n step(s) at seed $s; exactly one branch must run"
        local a; a=$(step_field "$t" 0 action)
        case "$a" in A|B|C) : ;; *) bad "select chose '$a' at seed $s, which is not one of its branches" ;; esac
        [ -n "$(step_field "$t" 0 group)" ] || bad "selected branch at seed $s carries no group id"
        first=${first:-$a}
        case " $chosen " in *" $a "*) : ;; *) chosen="$chosen $a" ;; esac
    done
    local k; k=$(printf '%s\n' $chosen | wc -l | tr -d ' ')
    [ "$k" = 3 ] || bad "over 40 seeds select reached only $k of 3 branches (saw:$chosen)"
    return 0
}

c40() {
    local s t seen=""
    for s in $SEEDS; do
        t=$(solve "$TMP/c40.pss" "$s")
        [ "$(nsteps "$t")" = 1 ] || bad "if-else produced $(nsteps "$t") step(s) at seed $s, expected 1"
        local a; a=$(step_field "$t" 0 action)
        case " $seen " in *" $a "*) : ;; *) seen="$seen $a" ;; esac
    done
    case " $seen " in *" A "*) : ;; *) bad "if-else never took the then-branch over 40 seeds (saw:$seen)" ;; esac
    case " $seen " in *" B "*) : ;; *) bad "if-else never took the else-branch over 40 seeds (saw:$seen)" ;; esac
    return 0
}

c41() {
    local s t seen=""
    for s in $SEEDS; do
        t=$(solve "$TMP/c41.pss" "$s")
        [ "$(nsteps "$t")" = 1 ] || bad "match produced $(nsteps "$t") step(s) at seed $s, expected 1"
        local a; a=$(step_field "$t" 0 action)
        case " $seen " in *" $a "*) : ;; *) seen="$seen $a" ;; esac
    done
    local a
    for a in A B C; do
        case " $seen " in *" $a "*) : ;; *) bad "match arm for $a was never reached over 40 seeds (saw:$seen)" ;; esac
    done
    return 0
}

c42() {
    local t; t=$(solve "$TMP/c42.pss")
    [ "$(nsteps "$t")" = 6 ] || bad "replicate(3) of a 2-statement body produced $(nsteps "$t") step(s), expected 6"
    [ "$(step_field "$t" 0 action)$(step_field "$t" 1 action)$(step_field "$t" 2 action)$(step_field "$t" 3 action)$(step_field "$t" 4 action)$(step_field "$t" 5 action)" = "ABABAB" ] \
        || bad "replicate copies are not in body order (saw $(step_field "$t" 0 action)$(step_field "$t" 1 action)$(step_field "$t" 2 action)$(step_field "$t" 3 action)$(step_field "$t" 4 action)$(step_field "$t" 5 action))"
    return 0
}

c43() {
    local t; t=$(solve "$TMP/c43.pss")
    local n; n=$(nsteps "$t")
    [ "$n" = 3 ] || bad "symbol \`two\` { do A; do B; } traversed by \`two;\` produced $n step(s), expected 3 (A, B, then C); the symbol expanded to nothing and the activity collapsed to '$(step_field "$t" 0 action)'"
    [ "$(step_field "$t" 0 action)/$(step_field "$t" 1 action)/$(step_field "$t" 2 action)" = "A/B/C" ] \
        || bad "symbol expansion produced $(step_field "$t" 0 action), $(step_field "$t" 1 action), $(step_field "$t" 2 action), expected A, B, C"
    return 0
}

c44() {
    local on off
    on=$(solve "$TMP/c44_on.pss"); off=$(solve "$TMP/c44_off.pss")
    [ "$(nsteps "$on")" = 3 ] || bad "bind model produced $(nsteps "$on") step(s), expected 3"
    local p1 p2 c
    p1=$(fobj "$on" 0 output out); p2=$(fobj "$on" 1 output out); c=$(fobj "$on" 2 input inp)
    [ -n "$p1" ] && [ -n "$p2" ] || bad "the producers did not allocate objects (obj '$p1', '$p2')"
    [ "$c" = "$p2" ] || bad "bind p2.out c.inp had no effect: the consumer input is bound to object #$c, which is the first producer's output (#$p1), not p2's (#$p2)"
    [ "$on" != "$off" ] || bad "bind p2.out c.inp left the trace identical to the model without it — the statement is silently dropped"
    return 0
}

c45() {
    local t; t=$(solve "$TMP/c45.pss")
    [ "$(nsteps "$t")" = 2 ] || bad "buffer producer/consumer produced $(nsteps "$t") step(s), expected 2"
    local o i
    o=$(fobj "$t" 0 output out); i=$(fobj "$t" 1 input inp)
    [ -n "$o" ] && [ "$o" != "-" ] || bad "the producer's output buffer was not allocated from the pool (obj '$o')"
    [ "$i" = "$o" ] || bad "the consumer's input buffer is object '$i', the producer's output is object '$o' — one buffer object must mediate both"
    return 0
}

c46() {
    local t; t=$(solve "$TMP/c46.pss")
    [ "$(nsteps "$t")" = 3 ] || bad "stream producer and two consumers produced $(nsteps "$t") step(s), expected 3"
    local first second
    first=$(fobj "$t" 1 input inp); second=$(fobj "$t" 2 input inp)
    [ -n "$first" ] && [ "$first" != "-" ] || bad "the first consumer of the stream got no object (obj '$first')"
    warn_has "$t" "flow-object ordering" \
        || bad "the second consumer of an already-consumed stream was not reported (no flow-object ordering warning); it silently took object #$second again"
    return 0
}

c47() {
    local t; t=$(solve "$TMP/c47.pss")
    [ "$(nsteps "$t")" = 2 ] || bad "state producer/consumer produced $(nsteps "$t") step(s), expected 2"
    local w r
    w=$(fval "$t" 0 "nxt.mode"); r=$(fval "$t" 1 "prv.mode")
    [ "$w" = 5 ] || bad "the writer's state field solved as '$w', expected 5"
    [ "$r" = "$w" ] || bad "the state object is not persisted: the writer set mode=$w, the reader of the same object solved mode=$r"
    return 0
}

c48() {
    local t; t=$(solve "$TMP/c48_order.pss")
    warn_has "$t" "flow-object ordering" \
        || bad "a consumer scheduled before any producer was not reported as a flow-object ordering violation"
    local i; i=$(fobj "$t" 0 input inp)
    [ "$i" = "-" ] || bad "a consumer with no preceding producer was bound to object #$i anyway"

    t=$(solve "$TMP/c48_value.pss")
    local o v
    o=$(fval "$t" 0 "out.size"); v=$(fval "$t" 1 "inp.size")
    [ "$o" = 7 ] || bad "the producer's output field solved as '$o', expected 7"
    [ "$(fobj "$t" 1 input inp)" = "$(fobj "$t" 0 output out)" ] \
        || bad "the consumer's input is not bound to the producer's object"
    [ "$v" = "$o" ] || bad "the input reference does not carry the producer's data: the bound object holds size=$o, the consumer's input field solved size=$v"
    return 0
}

c49() {
    local t; t=$(solve "$TMP/c49.pss")
    [ "$(nsteps "$t")" = 2 ] || bad "two parallel lockers produced $(nsteps "$t") step(s), expected 2"
    local a b
    a=$(claim "$t" 0); b=$(claim "$t" 1)
    [ "$a" != "-" ] && [ -n "$a" ] || bad "the lock reference did not resolve to a resource instance"
    [ "$a" != "$b" ] \
        || bad "two parallel lockers on a 4-instance pool both resolved to instance $a; distinct instances were never assigned"
    warn_has "$t" "contention" \
        && bad "resource contention reported for two lockers of a 4-instance pool (both on instance $a)"
    return 0
}

c50() {
    local t; t=$(solve "$TMP/c50.pss")
    [ "$(nsteps "$t")" = 2 ] || bad "two parallel lock references produced $(nsteps "$t") step(s), expected 2"
    local a b
    a=$(claim "$t" 0); b=$(claim "$t" 1)
    [ "$a" != "-" ] && [ -n "$a" ] || bad "the resource reference did not resolve to an instance"
    [ "$a" != "$b" ] \
        || bad "two parallel lock references both resolved to instance $a; a 4-instance pool must give them distinct instances"
    return 0
}

run() {
    local n=$1 name=$2 fn=$3
    : > "$TMP/reason"
    if ( "$fn" ); then
        printf 'PASS %s %s\n' "$n" "$name"
    else
        local why; why=$(cat "$TMP/reason")
        printf 'FAIL %s %s — %s\n' "$n" "$name" "${why:-assertion failed}"
        FAILS=$((FAILS + 1))
    fi
}

run 26 "Template value parameter declaration"  c26
run 27 "Template type parameter declaration"   c27
run 28 "Template type instantiation"           c28
run 29 "activity statement"                    c29
run 30 "Action traversal statement"            c30
run 31 "Activity sequence block"               c31
run 32 "Parallel statement"                    c32
run 33 "Schedule statement"                    c33
run 34 "Activity join specification"           c34
run 35 "Atomic block"                          c35
run 36 "repeat-count statement"                c36
run 37 "repeat-while statement"                c37
run 38 "foreach statement"                     c38
run 39 "select statement"                      c39
run 40 "if-else statement"                     c40
run 41 "match statement"                       c41
run 42 "replicate statement"                   c42
run 43 "symbol declaration"                    c43
run 44 "bind statement"                        c44
run 45 "buffer declaration"                    c45
run 46 "stream declaration"                    c46
run 47 "state declaration"                     c47
run 48 "Flow object reference"                 c48
run 49 "resource declaration"                  c49
run 50 "Resource object reference"             c50

echo
if [ "$FAILS" -eq 0 ]; then
    echo "25/25 constructs behave as the standard requires."
    exit 0
fi
echo "$FAILS of 25 constructs do not behave as the standard requires."
exit 1
