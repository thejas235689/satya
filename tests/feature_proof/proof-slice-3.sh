#!/bin/sh
# proof-slice-3.sh — assertion-based proof for slice 3 of the Satya v0.46.0
# feature audit: constructs 51-75 (constraints, coverage, monitors).
#
#   ./proof-slice-3.sh /path/to/satya
#
# Every verdict is re-derived by ASSERTING on observed behaviour — solved
# values, reachability over many seeds, reported coverage bin counts and
# monitor matches — never on whether a model parses. One line per construct:
#
#   PASS <n> <name>
#   FAIL <n> <name> — <reason>
#
# Exit 0 if every construct behaves as the standard requires, 1 if any does
# not, 2 if the binary argument is missing, not executable, or not Satya.
#
# All models are embedded below and written into a mktemp directory, so the
# script needs nothing on disk but itself.
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
case $BIN in
    /*) ;;
    *)  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")" ;;
esac
if ! "$BIN" help 2>&1 | grep -q 'Satya'; then
    echo "error: $BIN does not identify itself as Satya" >&2
    exit 2
fi

TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT INT TERM

# ---------------------------------------------------------------- helpers ---
run() { "$BIN" solve "$TMP/$1.pss" --action "$2" --seed "$3" 2>&1; }

# vals <model> <action> <seed> <"k1 k2 ..."> — the solved values, one line,
# "-" for a field that was not reported. One binary run, one awk.
vals() {
    run "$1" "$2" "$3" | awk -v keys="$4" '
        BEGIN { n = split(keys, k, " ") }
        /solved: / {
            sub(/.*solved: /, "")
            m = split($0, t, " ")
            for (i = 1; i <= m; i++) {
                p = index(t[i], "=")
                if (p > 0) v[substr(t[i], 1, p - 1)] = substr(t[i], p + 1)
            }
        }
        END { for (i = 1; i <= n; i++) printf "%s ", (k[i] in v ? v[k[i]] : "-"); printf "\n" }'
}
# "hit/declared" for a coverpoint, empty when the coverpoint is not reported
bins() {
    printf '%s\n' "$1" | awk -v cp="$2" \
        '$1==cp{for(i=1;i<=NF;i++) if($i=="bins"){print $(i+1); exit}}'
}
# hit count reported for a monitor, empty when it did not match
monhits() {
    printf '%s\n' "$1" | awk -v m="$2" '$1=="monitor" && $2==m {print $3; exit}'
}
# is a coverpoint or cross row present at all (crosses report no bins)
reported() {
    printf '%s\n' "$1" | awk -v cp="$2" '$1==cp{print "yes"; exit}'
}
nsteps() { printf '%s\n' "$1" | awk '$1=="steps"{print $3; exit}'; }
# "pool|obj" per allocation line, one per line
poolobjs() { printf '%s\n' "$1" | sed -n 's/.*from pool `\([^`]*\)` obj \(.*\)$/\1|\2/p'; }
# every value in the whitespace-separated list $1 lies in [lo,hi]; no "-"
inrange() {
    printf '%s\n' "$1" | awk -v lo="$2" -v hi="$3" \
        'BEGIN{ok=1;n=0}{for(i=1;i<=NF;i++){n++; if($i=="-"||$i+0<lo+0||$i+0>hi+0) ok=0}}
         END{exit (n>0 && ok)?0:1}'
}
# the whitespace-separated list $1 has no duplicates and no "-"
uniqok() {
    printf '%s\n' "$1" | awk '
        BEGIN{ok=1;n=0}
        {for(i=1;i<=NF;i++){n++; if($i=="-"||$i==""){ok=0;continue} if(seen[$i]++) ok=0}}
        END{exit (n>0 && ok)?0:1}'
}
# the list $1 holds exactly one distinct value
sameval() { printf '%s\n' "$1" | awk '{for(i=1;i<=NF;i++) s[$i]=1} END{exit (length(s)==1)?0:1}'; }

pass=0
fails=0
verdict() { # n name reason
    if [ -z "${3:-}" ]; then
        printf 'PASS %s %s\n' "$1" "$2"
        pass=$((pass + 1))
    else
        printf 'FAIL %s %s — %s\n' "$1" "$2" "$3"
        fails=$((fails + 1))
    fi
}
R=""
add() { if [ -z "$R" ]; then R="$1"; else R="$R; $1"; fi; }

# ----------------------------------------------------------------- models ---
cat > "$TMP/m51.pss" <<'PSS'
package p51 {
    struct pkt_s {
        rand int size;
        constraint size in [1..16];
    };
    component c51 {
        pool [3] pkt_s pkts;
        action item_a {
            output pkt_s p;
            constraint p.size in [1..8];
        };
        action top_a { activity { do item_a; do item_a; do item_a; }; };
        action over_a { activity { repeat (4) { do item_a; }; }; };
    };
};
PSS

cat > "$TMP/m52.pss" <<'PSS'
package p52 {
    struct pkt_s {
        rand int size;
        constraint size in [1..16];
    };
    component c52 {
        pool [2] pkt_s pb;
        pool [2] pkt_s pa;
        bind pa *;
        action item_a {
            output pkt_s p;
            constraint p.size in [1..8];
        };
        action top_a { activity { do item_a; do item_a; }; };
    };
};
PSS

cat > "$TMP/m53.pss" <<'PSS'
package p53 {
    struct pkt_s {
        rand int size;
        constraint size in [4..6];
    };
    component c53 {
        action item_a {
            rand pkt_s p;
            rand int n;
            constraint n in [0..15];
            constraint p.size in [0..15];
        };
        action named_a {
            rand int a;
            constraint a in [0..20];
            constraint narrow { a in [3..5]; };
        };
        action top_a { activity { do item_a; }; };
    };
};
PSS

cat > "$TMP/m54.pss" <<'PSS'
package p54 {
    component c54 {
        action sum_a {
            rand int a; rand int b;
            constraint a in [0..10];
            constraint b in [0..10];
            constraint { a + b == 10; }
        };
        action prod_a {
            rand int a; rand int b;
            constraint a in [0..10];
            constraint b in [0..10];
            constraint { a * b <= 20; }
        };
        action lt_a {
            rand int a; rand int b;
            constraint a in [0..10];
            constraint b in [0..10];
            constraint { a < b; }
        };
        action expr_a {
            rand int a; rand int b;
            constraint a in [0..10];
            constraint b in [0..10];
            constraint { a + b == 10; }
            constraint { a * b <= 20; }
            constraint { a < b; }
        };
    };
};
PSS

cat > "$TMP/m54b.pss" <<'PSS'
package p54b {
    component c54b {
        action top_a {
            rand int z;
            constraint z in [1, 4, 9..12, 20..];
        };
    };
};
PSS

cat > "$TMP/m55.pss" <<'PSS'
package p55 {
    enum kind_e { READ, WRITE, IDLE };
    component c55 {
        action impl_a {
            rand int len;
            rand kind_e kind;
            rand int burst;
            constraint len in [1..64];
            constraint kind == WRITE -> burst in [2..8];
            constraint kind == READ  -> burst == 0;
            constraint len > 32 -> burst <= 4;
        };
    };
};
PSS

cat > "$TMP/m56.pss" <<'PSS'
package p56 {
    component c56 {
        action cond_a {
            rand int len;
            rand int burst;
            constraint len in [0..64];
            constraint {
                if (len > 32) { burst in [0..4]; }
                else { burst in [10..20]; }
            }
        };
    };
};
PSS

cat > "$TMP/m57.pss" <<'PSS'
package p57 {
    component c57 {
        action fe_a {
            rand int payload[8];
            constraint foreach (payload[i]) { payload[i] in [0..15]; }
        };
        action fe_bound_a {
            rand int ids[6];
            constraint foreach (v : ids) { v in [100..110]; }
        };
    };
};
PSS

cat > "$TMP/m58.pss" <<'PSS'
// 58 — forall over a type that IS instantiated in the action. The body
// constrains the bound variable, so it means nothing unless forall expands.
package p58 {
    struct elem_s { rand int size; };
    component c58 {
        action fa_a {
            rand elem_s e;
            constraint e.size in [0..100];
            constraint forall (it : elem_s) { it.size < 5; };
        };
        action fa_b {
            rand elem_s e;
            constraint e.size in [0..100];
        };
    };
};
PSS

cat > "$TMP/m59.pss" <<'PSS'
package p59 {
    component c59 {
        action arr_a {
            rand int ids[4];
            constraint foreach (ids[i]) { ids[i] in [1..4]; }
            constraint unique { ids };
        };
        action scalars_a {
            rand int x; rand int y; rand int z;
            constraint x in [1..3];
            constraint y in [1..3];
            constraint z in [1..3];
            constraint unique { x, y, z };
        };
        action impossible_a {
            rand int five[5];
            constraint foreach (five[i]) { five[i] in [1..4]; }
            constraint unique { five };
        };
    };
};
PSS

cat > "$TMP/m60.pss" <<'PSS'
package p60 {
    struct my_struct {
        rand int in [0..3] attr1;
        constraint default attr1 == 0;
        rand int in [0..3] attr2;
        constraint attr1 < attr2;
    };
    component c60 {
        action my_action {
            rand my_struct s1;
            rand my_struct s2;
            constraint default s2.attr1 == 2;
            rand my_struct s3;
            constraint default disable s3.attr1;
            constraint s3.attr1 > 0;
        };
    };
};
PSS

cat > "$TMP/m61.pss" <<'PSS'
package p61 {
    component c61 {
        action dist_a {
            rand int pick;
            constraint dist pick in [0 := 1, 1 := 3, 10..12 :/ 6];
        };
    };
};
PSS

cat > "$TMP/m62.pss" <<'PSS'
package p62 {
    component c62 {
        action a1 { rand int v; constraint v in [0..9]; };
        action a2 { rand int v; constraint v in [0..9]; };
        action seq_a {
            activity { constraint sequence { a1, a2 }; do a2; do a1; };
        };
        action par_a {
            activity { parallel { do a1; do a2; }; constraint parallel { a1, a2 }; };
        };
    };
};
PSS

cat > "$TMP/m63.pss" <<'PSS'
// 63 — the covergroup samples its formal against the declared bins, so an
// in-range draw must hit and an out-of-range draw must not.
package p63 {
    component c63 {
        covergroup cg (int len) { coverpoint len { bins lo = [0..3]; } }
        action in_lo { rand int len; constraint len in [0..3]; };
        action in_hi { rand int len; constraint len in [4..7]; };
        action top_lo { activity { cover { do in_lo; } }; };
        action top_hi { activity { cover { do in_hi; } }; };
    };
};
PSS

cat > "$TMP/m64.pss" <<'PSS'
package p64 {
    component c64 {
        covergroup named_cg (int v) {
            coverpoint v { bins lo = [0..3]; bins hi = [4..7]; }
        }
        action item_a {
            rand int x;
            constraint x in [0..7];
            named_cg inst1 (x);
            covergroup { coverpoint x { bins lo = [0..3]; bins hi = [4..7]; } } inline_cg;
        };
        action top_a { activity { cover { do item_a; } }; };
    };
};
PSS

cat > "$TMP/m65.pss" <<'PSS'
// 65 — `coverpoint y iff (x > 3)` must sample only when the guard holds.
package p65 {
    component c65 {
        covergroup cg (int x, int y) {
            cp_x: coverpoint x { bins any = [0..7]; }
            coverpoint y iff (x > 3) { bins any = [0..7]; }
        }
        action lo_a { rand int x; rand int y; constraint x in [0..3]; constraint y in [0..7]; };
        action hi_a { rand int x; rand int y; constraint x in [4..7]; constraint y in [0..7]; };
        action top_lo { activity { cover { do lo_a; } }; };
        action top_hi { activity { cover { do hi_a; } }; };
    };
};
PSS

cat > "$TMP/m66.pss" <<'PSS'
package p66 {
    enum e_t { A, B, C, D };
    component c66 {
        covergroup cg (int v, e_t k, e_t k2) {
            coverpoint v {
                bins zero  = 0;
                bins low   = [1..3];
                bins evens = [4, 6, 8];
            }
            coverpoint k {
                bins all[4] = { A, B, C, D };
            }
            coverpoint k2 {
                bins four[] = [A, B, C, D];
            }
        }
        action top_a { activity { cover { do item_a; } }; };
        action item_a {
            rand int v;
            rand e_t k;
            rand e_t k2;
            constraint v in [0..9];
        };
    };
};
PSS

cat > "$TMP/m67.pss" <<'PSS'
package p67 {
    enum e_t { A, B };
    component c67 {
        covergroup cg (int len, e_t k) {
            coverpoint len { label lb; bins small = [0..3]; bins large = [4..7]; }
            coverpoint k   { label kb; }
            cross lb, kb   { label lxk; }
        }
        action top_a { activity { cover { do item_a; } }; };
        action item_a {
            rand int len;
            rand e_t k;
            constraint len in [0..7];
        };
    };
};
PSS

cat > "$TMP/m68.pss" <<'PSS'
// 68 — `cover <monitor>;` matches and samples; a covered monitor whose
// pattern never occurs must report nothing at all.
package p68 {
    component c68 {
        action item_a  { rand int v; constraint v in [6..7]; };
        action other_a { rand int w; constraint w in [0..3]; };
        monitor mon68 {
            item_a it;
            activity { do it; }
            covergroup cg (int v) { coverpoint v { bins hi = [6..7]; } }
        }
        monitor unbound {
            other_a ot;
            activity { do ot; }
            covergroup cgu (int w) { coverpoint w { bins any = [0..3]; } }
        }
        action top_a { activity { do item_a; cover mon68; } };
        action top_b { activity { do item_a; cover unbound; } };
    };
};
PSS

cat > "$TMP/m69.pss" <<'PSS'
// 69 — a monitor's activity matches its steps in order.
package p69 {
    component c69 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        monitor act2 {
            a x; b y;
            activity { do x; do y; }
            covergroup cg (int av) { coverpoint av { bins any = [1..3]; } }
        }
        action t_ab { activity { do a; do b; cover act2; } };
        action t_a  { activity { do a; cover act2; } };
        action t_ba { activity { do b; do a; cover act2; } };
    };
};
PSS

cat > "$TMP/m70.pss" <<'PSS'
// 70 — traversal by handle (`x;`), by type (`do a;`), and with an inline
// constraint (`do a with av == 2`), which must gate the match.
package p70 {
    component c70 {
        action a { rand int av; constraint av in [0..3]; };
        monitor by_handle {
            a x;
            activity { x; }
            covergroup cg (int av) { coverpoint av { bins any = [0..3]; } }
        }
        monitor by_type {
            activity { do a; }
            covergroup cg (int av) { coverpoint av { bins any = [0..3]; } }
        }
        monitor with_two {
            activity { do a with av == 2; }
        }
        action t_h { activity { do a; cover by_handle; } };
        action t_t { activity { do a; cover by_type; } };
        action t_w { activity { do a; cover with_two; } };
    };
};
PSS

cat > "$TMP/m71.pss" <<'PSS'
// 71 — sequence: the sub-scenarios match in the given order.
package p71 {
    component c71 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        action g { rand int gv; constraint gv in [8..9]; };
        monitor seq2 {
            a x; b y;
            activity { sequence { do x; do y; } }
            covergroup cg (int av) { coverpoint av { bins any = [1..3]; } }
        }
        monitor seq3 {
            a x; b y; g z;
            activity { sequence { do x; do y; do z; } }
        }
        action t_ab  { activity { do a; do b; cover seq2; } };
        action t_ba  { activity { do b; do a; cover seq2; } };
        action t_abg { activity { do a; do b; do g; cover seq3; } };
        action t_agb { activity { do a; do g; do b; cover seq3; } };
    };
};
PSS

cat > "$TMP/m72.pss" <<'PSS'
// 72 — concat requires the sub-scenarios to be contiguous.
package p72 {
    component c72 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        action g { rand int gv; constraint gv in [8..9]; };
        monitor cat2 {
            a x; b y;
            activity { concat { do x; do y; } }
            covergroup cg (int av) { coverpoint av { bins any = [1..3]; } }
        }
        action t_contig { activity { do a; do b; cover cat2; } };
        action t_gap    { activity { do a; do g; do b; cover cat2; } };
    };
};
PSS

cat > "$TMP/m73.pss" <<'PSS'
// 73 — eventually matches its sub-scenario at or after the current point.
package p73 {
    component c73 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        action g { rand int gv; constraint gv in [8..9]; };
        monitor ev {
            a x; b y;
            activity { concat { do x; eventually do y; } }
            covergroup cg (int av) { coverpoint av { bins any = [1..3]; } }
        }
        action t_ev_ok  { activity { do a; do g; do b; cover ev; } };
        action t_ev_bad { activity { do b; do g; do a; cover ev; } };
    };
};
PSS

cat > "$TMP/m74.pss" <<'PSS'
// 74 — overlap requires a time instant where all branches are active, so a
// strictly sequential trace must not realize it.
package p74 {
    component c74 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        monitor ov {
            a x; b y;
            activity { overlap { do x; do y; } }
            covergroup cg (int av) { coverpoint av { bins any = [1..3]; } }
        }
        action t_seq { activity { do a; do b; cover ov; } };
    };
};
PSS

cat > "$TMP/m75.pss" <<'PSS'
// 75 — select takes an alternative arm, and each arm samples its own covergroup.
package p75 {
    component c75 {
        action a { rand int av; constraint av in [1..3]; };
        action b { rand int bv; constraint bv in [4..7]; };
        action g { rand int gv; constraint gv in [8..9]; };
        monitor sel {
            a x; b y;
            activity { select { do x; do y; } }
            covergroup cga (int av) { coverpoint av { bins any = [1..3]; } }
            covergroup cgb (int bv) { coverpoint bv { bins any = [4..7]; } }
        }
        action t_arm_a { activity { do a; do g; cover sel; } };
        action t_arm_b { activity { do b; do g; cover sel; } };
    };
};
PSS

# ------------------------------------------------------------ construct 51 ---
R=""
o=$(run m51 top_a 1)
[ "$(nsteps "$o")" = 3 ] || add "expected 3 steps, saw $(nsteps "$o")"
got=$(poolobjs "$o" | tr '\n' ' ')
[ "$got" = "pkts|#0 pkts|#1 pkts|#2 " ] || add "pool objects not allocated #0,#1,#2 (saw: $got)"
inrange "$(vals m51 top_a 1 'p.size')" 1 8 || add "p.size outside the action constraint [1..8]"
o2=$(run m51 over_a 1)
[ "$(nsteps "$o2")" = 4 ] || add "4-allocation run gave $(nsteps "$o2") steps"
printf '%s\n' "$o2" | grep -q 'exhausted' || add "4th allocation not refused: no pool-exhausted warning"
printf '%s\n' "$o2" | grep -q 'obj -' || add "4th allocation still reports a pool object"
[ "$(poolobjs "$o2" | grep -c 'pkts|#')" = 3 ] || add "pool handed out more than 3 objects"
verdict 51 "Pool instantiation" "$R"

# ------------------------------------------------------------ construct 52 ---
R=""
o=$(run m52 top_a 1)
from=$(poolobjs "$o" | cut -d'|' -f1 | sort -u | tr '\n' ' ')
[ "$from" = "pa " ] || add "bind pa *; ignored: allocations came from pool(s) [$from], not [pa]"
verdict 52 "Static bind directives" "$R"

# ------------------------------------------------------------ construct 53 ---
R=""
bad=0
for s in $(seq 1 40); do
    v=$(vals m53 top_a "$s" 'p.size n')
    set -- $v
    { [ "$1" != "-" ] && [ "$1" -ge 4 ] && [ "$1" -le 6 ]; } || bad=$((bad + 1))
    { [ "$2" != "-" ] && [ "$2" -ge 0 ] && [ "$2" -le 15 ]; } || bad=$((bad + 1))
    v=$(vals m53 named_a "$s" 'a')
    { [ "$v" != "- " ] && [ "${v% }" -ge 3 ] && [ "${v% }" -le 5 ]; } || bad=$((bad + 1))
done
[ "$bad" = 0 ] || add "$bad of 120 samples escaped the member constraints (p.size must stay in 4..6, named a in 3..5)"
verdict 53 "Member constraint declaration" "$R"

# ------------------------------------------------------------ construct 54 ---
R=""
bad=0
for s in $(seq 1 40); do
    v=$(vals m54 expr_a "$s" 'a b')
    set -- $v
    if [ "$1" = "-" ] || [ "$2" = "-" ] || [ $(( $1 + $2 )) -ne 10 ] || [ $(( $1 * $2 )) -gt 20 ] || [ "$1" -ge "$2" ]; then
        bad=$((bad + 1))
    fi
done
[ "$bad" = 0 ] || add "expr_a violates a+b==10 / a*b<=20 / a<b on $bad of 40 seeds"
bad=0
for s in $(seq 1 40); do
    v=$(vals m54 prod_a "$s" 'a b')
    set -- $v
    if [ "$1" = "-" ] || [ "$2" = "-" ] || [ $(( $1 * $2 )) -gt 20 ]; then bad=$((bad + 1)); fi
done
[ "$bad" = 0 ] || add "prod_a violates a*b<=20 on $bad of 40 seeds"
# mixed open_range_list: every member must be REACHABLE, not merely legal
: > "$TMP/or.txt"
for s in $(seq 1 200); do
    vals m54b top_a "$s" 'z' >> "$TMP/or.txt"
done
missing=$(awk '
    { v = $1 + 0; if ($1 != "-") { seen[v] = 1; n++ } }
    END {
        split("1 4 9 10 11 12", w, " ")
        m = 0
        for (i = 1; i <= 6; i++) if (!(w[i] + 0 in seen)) m++
        hi = 0
        for (v in seen) if (v + 0 >= 20) hi = 1
        if (!hi) m++
        printf "%d", m
    }' "$TMP/or.txt")
[ "$missing" = 0 ] || add "mixed open_range_list [1, 4, 9..12, 20..]: $missing of 7 members unreachable over 200 seeds (only [$(sort -n "$TMP/or.txt" | uniq | awk '{printf "%s%s", (n++?",":""), $1}')] ever solved)"
verdict 54 "Expression constraint" "$R"

# ------------------------------------------------------------ construct 55 ---
R=""
bad=0; nk=0
for s in $(seq 1 50); do
    v=$(vals m55 impl_a "$s" 'kind burst len')
    set -- $v
    k=$1; b=$2; l=$3
    [ "$k" = "-" ] || nk=$((nk + 1))
    if [ "$k" = "-" ] || [ "$b" = "-" ] || [ "$l" = "-" ]; then bad=$((bad + 1)); continue; fi
    [ "$k" = 1 ] && { [ "$b" -ge 2 ] && [ "$b" -le 8 ] || bad=$((bad + 1)); }
    [ "$k" = 0 ] && { [ "$b" = 0 ] || bad=$((bad + 1)); }
    [ "$l" -gt 32 ] && { [ "$b" -le 4 ] || bad=$((bad + 1)); }
    { [ "$l" -ge 1 ] && [ "$l" -le 64 ]; } || bad=$((bad + 1))
done
[ "$nk" = 50 ] || add "implication model did not solve on every seed"
[ "$bad" = 0 ] || add "implication violated on $bad of 50 seeds (WRITE->2..8, READ->0, len>32->burst<=4)"
verdict 55 "Implication constraint" "$R"

# ------------------------------------------------------------ construct 56 ---
R=""
bad=0; lo=0; hi=0
for s in $(seq 1 50); do
    v=$(vals m56 cond_a "$s" 'len burst')
    set -- $v
    l=$1; b=$2
    if [ "$l" = "-" ] || [ "$b" = "-" ]; then bad=$((bad + 1)); continue; fi
    if [ "$l" -gt 32 ]; then hi=$((hi + 1)); else lo=$((lo + 1)); fi
    if [ "$l" -gt 32 ]; then
        { [ "$b" -ge 0 ] && [ "$b" -le 4 ]; } || bad=$((bad + 1))
    else
        { [ "$b" -ge 10 ] && [ "$b" -le 20 ]; } || bad=$((bad + 1))
    fi
done
[ "$lo" -gt 0 ] && [ "$hi" -gt 0 ] || add "only one branch reachable (len>32 on $hi seeds, len<=32 on $lo)"
[ "$bad" = 0 ] || add "conditional branch value wrong on $bad of 50 seeds"
verdict 56 "Conditional constraint" "$R"

# ------------------------------------------------------------ construct 57 ---
R=""
bad=0; sz=0
for s in $(seq 1 40); do
    v=$(vals m57 fe_a "$s" 'payload.size payload[0] payload[1] payload[2] payload[3] payload[4] payload[5] payload[6] payload[7]')
    [ "${v%% *}" = 8 ] || sz=$((sz + 1))
    set -- $v; shift
    inrange "$*" 0 15 || bad=$((bad + 1))
    v=$(vals m57 fe_bound_a "$s" 'ids.size ids[0] ids[1] ids[2] ids[3] ids[4] ids[5]')
    [ "${v%% *}" = 6 ] || sz=$((sz + 1))
    set -- $v; shift
    inrange "$*" 100 110 || bad=$((bad + 1))
done
[ "$sz" = 0 ] || add "array extent wrong on $sz runs (expected payload[8] and ids[6])"
[ "$bad" = 0 ] || add "foreach element outside its range on $bad of 80 runs"
verdict 57 "foreach constraint" "$R"

# ------------------------------------------------------------ construct 58 ---
R=""
bad=0; same=0
for s in $(seq 1 20); do
    va=$(vals m58 fa_a "$s" 'e.size')
    vb=$(vals m58 fa_b "$s" 'e.size')
    [ "${va% }" = "-" ] && bad=$((bad + 1))
    [ "${va% }" = "${vb% }" ] && same=$((same + 1))
    { [ "${va% }" != "-" ] && [ "${va% }" -lt 5 ]; } || bad=$((bad + 1))
done
[ "$bad" = 0 ] || add "forall (it : elem_s) { it.size < 5; } not expanded: e.size >= 5 on $bad of 20 seeds"
[ "$same" = 20 ] || add "forall model differs from the unconstrained one on $((20 - same)) seeds"
verdict 58 "forall constraint" "$R"

# ------------------------------------------------------------ construct 59 ---
R=""
bad=0
for s in $(seq 1 60); do
    uniqok "$(vals m59 arr_a "$s" 'ids[0] ids[1] ids[2] ids[3]')" || bad=$((bad + 1))
    uniqok "$(vals m59 scalars_a "$s" 'x y z')" || bad=$((bad + 1))
done
[ "$bad" = 0 ] || add "unique not enforced on $bad of 120 runs (duplicate values)"
o=$(run m59 impossible_a 1)
sat=$(printf '%s\n' "$o" | sed -n 's/.*constraints: \([0-9][0-9]*\)\/\([0-9][0-9]*\).*/\1 \2/p' | head -1)
if [ -z "$sat" ]; then
    add "unsatisfiable unique set: no constraint report"
elif [ "${sat% *}" -ge "${sat#* }" ]; then
    add "unsatisfiable unique set (5 values over 1..4) reported as ${sat% *}/${sat#* } satisfied (faked)"
fi
verdict 59 "unique constraint" "$R"

# ------------------------------------------------------------ construct 60 ---
R=""
bad=0
for s in $(seq 1 40); do
    v=$(vals m60 my_action "$s" 's1.attr1 s2.attr1 s3.attr1 s1.attr2 s2.attr2 s3.attr2')
    set -- $v
    if [ "$1" = "-" ] || [ "$2" = "-" ] || [ "$3" = "-" ]; then bad=$((bad + 1)); continue; fi
    [ "$1" = 0 ] || bad=$((bad + 1))
    [ "$2" = 2 ] || bad=$((bad + 1))
    { [ "$3" -ge 1 ] && [ "$3" -le 2 ]; } || bad=$((bad + 1))
    { [ "$1" -lt "$4" ] && [ "$2" -lt "$5" ] && [ "$3" -lt "$6" ]; } || bad=$((bad + 1))
done
[ "$bad" = 0 ] || add "default-constraint expectations violated on $bad of 40 seeds (want s1.attr1=0, s2.attr1=2, s3.attr1 in 1..2, attr1<attr2)"
verdict 60 "Default constraints" "$R"

# ------------------------------------------------------------ construct 61 ---
R=""
: > "$TMP/dist.txt"
for s in $(seq 1 200); do
    vals m61 dist_a "$s" 'pick' >> "$TMP/dist.txt"
done
d=$(awk '
    { if ($1 == "-") { n++; bad = 1; next } c[$1 + 0]++; n++ }
    END {
        want[0] = 10; want[1] = 30; want[10] = 20; want[11] = 20; want[12] = 20
        msg = ""
        if (bad) msg = msg "some seeds did not solve; "
        for (v in c) if (!(v in want)) msg = msg "value " v " outside the declared set; "
        for (v in want) {
            obs = (v in c) ? 100 * c[v] / n : 0
            dev = obs - want[v]; if (dev < 0) dev = -dev
            if (dev > 6) msg = msg sprintf("value %d observed %.1f%% (want %d%%, %.1fpp off); ", v, obs, want[v], dev)
        }
        print msg
    }' "$TMP/dist.txt")
[ -z "$d" ] || add "dist 1:3:2 over 200 seeds out of tolerance: $d"
verdict 61 "Distribution directive" "$R"

# ------------------------------------------------------------ construct 62 ---
R=""
o=$(run m62 seq_a 1)
order=$(printf '%s\n' "$o" | awk '/^ +[0-9]+\. /{print $2}' | tr '\n' ' ')
[ "$order" = "a1 a2 " ] || add "constraint sequence {a1, a2} ignored: activity lists a2,a1 and the trace keeps [$order]"
verdict 62 "scheduling constraint statement" "$R"

# ------------------------------------------------------------ construct 63 ---
R=""
o=$(run m63 top_lo 1)
[ "$(bins "$o" cg.len)" = "1/1" ] || add "in-range draw did not hit the declared bin (cg.len bins=$(bins "$o" cg.len))"
o=$(run m63 top_hi 1)
[ "$(bins "$o" cg.len)" = "0/1" ] || add "out-of-range draw hit the bin (cg.len bins=$(bins "$o" cg.len))"
verdict 63 "covergroup declaration" "$R"

# ------------------------------------------------------------ construct 64 ---
R=""
o=$(run m64 top_a 1)
reported=$(printf '%s\n' "$o" | awk '$3=="value(s)"{printf "%s ", $1}')
printf '%s\n' "$reported" | grep -q 'inst1' || add "named_cg inst1 (x) binds nothing and never samples: only [$reported] is reported"
verdict 64 "covergroup instantiation" "$R"

# ------------------------------------------------------------ construct 65 ---
R=""
o=$(run m65 top_lo 1)
[ -n "$(bins "$o" cg.cp_x)" ] || add "plain coverpoint cp_x not reported"
[ -z "$(bins "$o" cg.y)" ] || add "coverpoint y iff (x > 3) sampled while x=$(vals m65 top_lo 1 'x' | tr -d ' ')"
o=$(run m65 top_hi 1)
[ "$(bins "$o" cg.y)" = "1/1" ] || add "coverpoint y iff (x > 3) not sampled while x=$(vals m65 top_hi 1 'x' | tr -d ' ')"
verdict 65 "coverpoint declaration" "$R"

# ------------------------------------------------------------ construct 66 ---
R=""
o=$(run m66 top_a 1)
v=$(bins "$o" cg.v); k=$(bins "$o" cg.k); k2=$(bins "$o" cg.k2)
[ "${v##*/}" = 3 ] || add "coverpoint v declares ${v##*/} bins, expected 3 (zero, low, evens)"
[ "${k##*/}" = 4 ] || add "bins all[4] = {A,B,C,D} declares ${k##*/} bin(s), not 4 (reported bins $k)"
[ "${k2##*/}" = 4 ] || add "bins four[] = [A,B,C,D] declares ${k2##*/} bin(s), not 4 (reported bins $k2)"
verdict 66 "bins declaration" "$R"

# ------------------------------------------------------------ construct 67 ---
R=""
o=$(run m67 top_a 1)
[ -n "$(reported "$o" cg.lxk)" ] || add "cross lb, kb { label lxk; } missing from the report"
[ -n "$(reported "$o" cg.lb)" ] || add "crossed coverpoint lb not reported"
[ -n "$(reported "$o" cg.kb)" ] || add "crossed coverpoint kb not reported"
printf '%s\n' "$o" | awk '$1=="cg.lxk"{for(i=1;i<=NF;i++) if($i=="hit(s)") ok=1} END{exit ok?0:1}' \
    || add "cross lxk reported but never sampled"
verdict 67 "cross declaration" "$R"

# ------------------------------------------------------------ construct 68 ---
R=""
o=$(run m68 top_a 1)
[ "$(monhits "$o" mon68)" = 1 ] || add "cover mon68; did not match the executed step (hits=$(monhits "$o" mon68))"
[ "$(bins "$o" cg.v)" = "1/1" ] || add "monitor covergroup not sampled over the matched step (cg.v bins=$(bins "$o" cg.v))"
o=$(run m68 top_b 1)
printf '%s\n' "$o" | grep -q 'monitor unbound' && add "a monitor whose pattern never occurs was reported as matched"
printf '%s\n' "$o" | grep -q 'cgu.w' && add "a monitor whose pattern never occurs sampled its covergroup"
verdict 68 "Cover statement and monitor declaration" "$R"

# ------------------------------------------------------------ construct 69 ---
R=""
o=$(run m69 t_ab 1)
[ "$(monhits "$o" act2)" = 1 ] || add "activity { do x; do y; } did not match trace a,b (hits=$(monhits "$o" act2))"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "matched monitor did not sample cg.av"
for a in t_a t_ba; do
    o=$(run m69 "$a" 1)
    printf '%s\n' "$o" | grep -q 'monitor act2' && add "activity { do x; do y; } matched the $a trace, which does not realize the pattern"
done
verdict 69 "Monitor activity" "$R"

# ------------------------------------------------------------ construct 70 ---
R=""
o=$(run m70 t_h 1)
[ "$(monhits "$o" by_handle)" = 1 ] || add "handle traversal (x;) did not match (hits=$(monhits "$o" by_handle))"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "handle traversal matched but did not sample its covergroup"
o=$(run m70 t_t 1)
[ "$(monhits "$o" by_type)" = 1 ] || add "type traversal (do a;) did not match (hits=$(monhits "$o" by_type))"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "type traversal matched but did not sample its covergroup"
wm=0; tm=0
for s in $(seq 1 12); do
    o=$(run m70 t_w "$s")
    a=$(vals m70 t_w "$s" 'av' | tr -d ' ')
    [ "$a" = "-" ] && { tm=$((tm + 1)); continue; }
    if [ "$(monhits "$o" with_two)" = 1 ]; then
        wm=$((wm + 1))
        [ "$a" = 2 ] || tm=$((tm + 1))
    else
        [ "$a" != 2 ] || tm=$((tm + 1))
    fi
done
[ "$tm" = 0 ] || add "do a with av == 2 disagreed with the solved av on $tm of 12 seeds"
[ "$wm" -gt 0 ] || add "do a with av == 2 never matched, even when av was 2"
verdict 70 "Action traversal statement" "$R"

# ------------------------------------------------------------ construct 71 ---
R=""
o=$(run m71 t_ab 1)
[ "$(monhits "$o" seq2)" = 1 ] || add "sequence { do x; do y; } did not match trace a,b"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "sequence match did not sample its covergroup"
o=$(run m71 t_ba 1)
printf '%s\n' "$o" | grep -q 'monitor seq2' && add "sequence { do x; do y; } matched trace b,a — order not enforced"
o=$(run m71 t_abg 1)
[ "$(monhits "$o" seq3)" = 1 ] || add "3-member sequence did not match trace a,b,g"
o=$(run m71 t_agb 1)
printf '%s\n' "$o" | grep -q 'monitor seq3' && add "3-member sequence matched trace a,g,b — order not enforced"
verdict 71 "Sequence monitor activity statement" "$R"

# ------------------------------------------------------------ construct 72 ---
R=""
o=$(run m72 t_contig 1)
[ "$(monhits "$o" cat2)" = 1 ] || add "concat { do x; do y; } did not match the contiguous trace a,b"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "concat match did not sample its covergroup"
o=$(run m72 t_gap 1)
printf '%s\n' "$o" | grep -q 'monitor cat2' && add "concat matched trace a,g,b — a gap was accepted where contiguity is required"
verdict 72 "Concat monitor activity statement" "$R"

# ------------------------------------------------------------ construct 73 ---
R=""
o=$(run m73 t_ev_ok 1)
[ "$(monhits "$o" ev)" = 1 ] || add "concat { do x; eventually do y; } did not match trace a,g,b"
[ "$(bins "$o" cg.av)" = "1/1" ] || add "eventually match did not sample its covergroup"
o=$(run m73 t_ev_bad 1)
printf '%s\n' "$o" | grep -q 'monitor ev' && add "eventually matched trace b,g,a, where the eventual sub-scenario precedes the start"
verdict 73 "Eventually monitor activity statement" "$R"

# ------------------------------------------------------------ construct 74 ---
R=""
o=$(run m74 t_seq 1)
printf '%s\n' "$o" | grep -q 'monitor ov' \
    && add "overlap { do x; do y; } matched the strictly sequential trace a,b: no time instant has both branches active, so the branches are matched independently"
verdict 74 "Overlap monitor activity statement" "$R"

# ------------------------------------------------------------ construct 75 ---
R=""
o=$(run m75 t_arm_a 1)
[ "$(monhits "$o" sel)" = 1 ] || add "select { do x; do y; } did not match the first arm (trace a,g)"
[ "$(bins "$o" cga.av)" = "1/1" ] || add "first arm matched but cga was not sampled (cga.av=$(bins "$o" cga.av))"
[ -z "$(bins "$o" cgb.bv)" ] || add "first arm sampled cgb, the covergroup of the arm that did not match"
o=$(run m75 t_arm_b 1)
[ "$(monhits "$o" sel)" = 1 ] || add "select { do x; do y; } did not match the second arm (trace b,g)"
[ "$(bins "$o" cgb.bv)" = "1/1" ] || add "second arm matched but cgb was not sampled (cgb.bv=$(bins "$o" cgb.bv))"
verdict 75 "Select monitor activity statement" "$R"

# ---------------------------------------------------------------- summary ---
printf '\n%d passed, %d failed, %d constructs\n' "$pass" "$fails" "$((pass + fails))"
[ "$fails" = 0 ] && exit 0
exit 1
