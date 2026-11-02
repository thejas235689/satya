#!/bin/sh
# proof-slice-1.sh — assertion-based re-derivation of the slice-1 verdicts
# (PSS constructs 1-25: literals, data types, components) for Satya v0.46.0.
#
#   ./proof-slice-1.sh /path/to/satya
#
# Unlike tests/feature_proof/run_all.sh — which prints what the tool does and
# leaves the judgement to a reader — every construct here is ASSERTED. Each
# assertion reads solved values out of the `json` backend (never the trace text,
# never "it parsed") and compares them against what PSS requires:
#
#   * a field constrained `in [10..20]` is checked in range over 20 seeds
#   * a `bit[8]` field is checked never to exceed 255
#   * a `bool` is checked to be 0 or 1
#   * a `float32` is checked to hold a binary32 bit pattern, not a binary64 one
#   * an aggregate/set is checked for the elements and the cardinality it must have
#
# The models are embedded below as heredocs and written to a mktemp dir at run
# time, so this script needs nothing on disk but itself.
#
# Verdict mapping: the audit (docs/FEATURE-AUDIT.md) uses four verdicts
# (PASS / PARTIAL-OK / PARTIAL-WORSE / FAIL); this script has two, because the
# question it answers is binary — does the construct behave as the standard
# requires? A construct is PASS only if the audit's PASS would also be right.
# The audit's finer verdict is carried in the reason text of every FAIL line so
# the two records can be reconciled line by line.
#
# Exit: 0 if every assertion passes, 1 if any fails, 2 if the binary is
# unusable (fail closed: nothing runs).
set -u

BIN=${1:-}
SEEDS=20          # seeds per randomised assertion

die_usage() {
    echo "usage: $0 /path/to/satya" >&2
    exit 2
}

# ---------------------------------------------------------------- fail closed
[ -n "$BIN" ] || { echo "error: no binary given" >&2; die_usage; }
[ -e "$BIN" ] || { echo "error: no such file: $BIN" >&2; exit 2; }
[ -f "$BIN" ] || { echo "error: not a regular file: $BIN" >&2; exit 2; }
[ -x "$BIN" ] || { echo "error: not executable: $BIN" >&2; exit 2; }

# Identity check: must be Satya, not merely an executable. `satya help` prints a
# banner naming the tool and the `parse`/`solve` commands.
ident=$("$BIN" help 2>/dev/null | head -20)
case $ident in
    *Satya*) : ;;
    *) echo "error: $BIN does not identify as Satya (no help banner)" >&2
       exit 2 ;;
esac
case $ident in
    *parse*|*solve*) : ;;
    *) echo "error: $BIN does not look like Satya (no parse/solve commands)" >&2
       exit 2 ;;
esac

TMP=$(mktemp -d "${TMPDIR:-/tmp}/proof-slice-1.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT INT TERM

OUT=$TMP/out.json
GLOBAL_FAIL=0

# ------------------------------------------------------------------ utilities
# jget KEY — print the value of KEY from the `fields` (or `strings`) object of
# the last json run, or __ABSENT__. Satya's json backend pretty-prints one
# member per line, which is what makes this exact (no heuristic matching).
jget() {
    awk -v k="$1" '
        { s = $0; sub(/^[ \t]+/, "", s)
          if (substr(s,1,1) != "\"") next
          i = index(substr(s,2), "\"")
          if (i == 0) next
          key = substr(s, 2, i-1)
          if (key != k) next
          rest = substr(s, i+2)
          sub(/^[ \t]*:[ \t]*/, "", rest)
          sub(/,$/, "", rest)
          print rest; exit }' "$OUT"
}

# jnum KEY — numeric value, or __ABSENT__.
jnum() {
    v=$(jget "$1")
    [ -n "$v" ] || { echo "__ABSENT__"; return; }
    echo "$v"
}

# jstr KEY — string value with surrounding quotes stripped, or __ABSENT__.
jstr() {
    v=$(jget "$1")
    [ -n "$v" ] || { echo "__ABSENT__"; return; }
    case $v in
        \"*\") v=${v#\"}; v=${v%\"} ;;
    esac
    echo "$v"
}

# err1 — first line of the last diagnostic, with the scratch path stripped.
err1() { head -1 "$TMP/err.txt" | sed "s|$TMP/||g"; }

# run MODEL ACTION SEED — solve one model, leaving the json in $OUT.
# Returns non-zero if the tool refused the model.
run() {
    "$BIN" gen "$TMP/$1.pss" --backend json --action "$2" --seed "$3" \
        >"$OUT" 2>"$TMP/err.txt"
}

# step_field MODEL ACTION SEED KEY -> value, running the model if needed
solve_get() {
    _m=$1; _a=$2; _s=$3; _k=$4
    if run "$_m" "$_a" "$_s"; then jnum "$_k"; else echo "__ERROR__"; fi
}
solve_str() {
    _m=$1; _a=$2; _s=$3; _k=$4
    if run "$_m" "$_a" "$_s"; then jstr "$_k"; else echo "__ERROR__"; fi
}

# ------------------------------------------------------- per-construct record
CNUM=0; CNAME=""; CFAIL=""

begin() { CNUM=$1; CNAME=$2; CFAIL=""; }

# fail REASON — record a failure reason for the construct being tested.
fail() { CFAIL="${CFAIL:+$CFAIL; }$1"; }

# eq KEY EXPECTED — assert the last run's KEY equals EXPECTED.
eq() {
    _got=$(jnum "$1")
    if [ "$_got" != "$2" ]; then fail "$1=$_got (expected $2)"; fi
}
eqs() {
    _got=$(jstr "$1")
    if [ "$_got" != "$2" ]; then fail "$1=\"$_got\" (expected \"$2\")"; fi
}

# eq_in KEY SEED ACTION MODEL CANDIDATES... — assert over seeds
# in_range MODEL ACTION KEY LO HI SEEDS — assert LO <= value <= HI for every seed
in_range() {
    _m=$1; _a=$2; _k=$3; _lo=$4; _hi=$5; _n=${6:-$SEEDS}; _bad=""
    _s=1
    while [ "$_s" -le "$_n" ]; do
        _v=$(solve_get "$_m" "$_a" "$_s" "$_k")
        case $_v in
            __ABSENT__|__ERROR__) _bad="$_v"; break ;;
        esac
        if [ "$_v" -lt "$_lo" ] 2>/dev/null || [ "$_v" -gt "$_hi" ] 2>/dev/null; then
            _bad="$_v"; break
        fi
        _s=$((_s + 1))
    done
    if [ -n "$_bad" ]; then
        fail "$_k=$_bad out of [$_lo..$_hi] at seed $_s"
    fi
}

# in_set MODEL ACTION KEY MEMBERS... — assert the value is one of MEMBERS
in_set() {
    _m=$1; _a=$2; _k=$3; shift 3
    _n=${SEEDS}; _s=1; _bad=""
    while [ "$_s" -le "$_n" ]; do
        _v=$(solve_get "$_m" "$_a" "$_s" "$_k")
        _ok=0
        for _cand in "$@"; do
            [ "$_v" = "$_cand" ] && _ok=1
        done
        if [ "$_ok" -ne 1 ]; then _bad="$_v"; break; fi
        _s=$((_s + 1))
    done
    if [ -n "$_bad" ]; then
        _list=$(printf '%s,' "$@" | sed 's/,$//')
        fail "$_k=$_bad not in {$_list} at seed $_s"
    fi
}

end() {
    if [ -z "$CFAIL" ]; then
        printf 'PASS %s %s\n' "$CNUM" "$CNAME"
    else
        printf 'FAIL %s %s — %s\n' "$CNUM" "$CNAME" "$CFAIL"
        GLOBAL_FAIL=1
    fi
}

# =============================================================== the models
# One minimal model per construct. Names are mNN (or mNNa/mNNb) so a construct
# can carry more than one probe.

cat >"$TMP/m01.pss" <<'PSS'
package m01 {
    component c {
        action a {
            int dec; int b4; int h8; int o8; int d64;
            int sd;  int sh; int us; int p; int neg;
            exec pre_solve {
                dec = 42; b4 = 4'b1010; h8 = 8'hFF; o8 = 8'o17; d64 = 64'd2;
                us = 1_000_000; p = 2 ** 10; neg = -42;
                sd = 4'sd12; sh = 16'sH_FF;
            }
        };
    };
};
PSS

cat >"$TMP/m02.pss" <<'PSS'
package m02 {
    component c {
        action a {
            string lit; string copy; string joined; string empty_s; string long3;
            int len; int len_empty; int len_long;
            exec pre_solve {
                lit = "hello"; copy = lit; joined = lit + "!"; empty_s = "";
                long3 = "ab" + "cd" + "ef";
                len = lit.length(); len_empty = empty_s.length();
                len_long = long3.length();
            }
        };
    };
};
PSS

cat >"$TMP/m03.pss" <<'PSS'
package m03 {
    component c {
        action a {
            int arr[3]; list<int> li; int big[5];
            int r0; int r1; int r2; int l0; int l1; int b4v; int lsz;
            exec pre_solve {
                arr = {10, 20, 30};
                r0 = arr[0]; r1 = arr[1]; r2 = arr[2];
                li = {7, 8};
                l0 = li[0]; l1 = li[1]; lsz = li.size();
                big = {1,2,3,4,5}; b4v = big[4];
            }
        };
    };
};
PSS

cat >"$TMP/m04.pss" <<'PSS'
package m04 {
    component c {
        action a {
            int arr[3];
            int sz_before; int sz_after;
            exec pre_solve {
                arr = {1, 2, 3};
                sz_before = arr.size();
                arr = {};
                sz_after  = arr.size();
            }
        };
    };
};
PSS

cat >"$TMP/m05.pss" <<'PSS'
package m05 {
    component c {
        action a {
            rand int x; rand int y;
            constraint x in [1, 2, 3];
            constraint y in [7, 11];
        };
    };
};
PSS

cat >"$TMP/m06.pss" <<'PSS'
package m06 {
    component c {
        action a {
            map<int,int> m;
            int sz_lit; int got; int sz_ins; int got3;
            exec pre_solve {
                m = {1:10, 2:20};
                sz_lit = m.size();
                got = m[1];
                m.insert(3, 30);
                sz_ins = m.size();
                got3 = m[3];
            }
        };
    };
};
PSS

cat >"$TMP/m07a.pss" <<'PSS'
package m07a {
    struct pt_s { int x; int y; };
    component c {
        action a {
            pt_s p;
            exec pre_solve { p = '{x : 3, y : 4}; }
        };
    };
};
PSS

cat >"$TMP/m07b.pss" <<'PSS'
package m07b {
    struct pt_s { int x; int y; };
    component c {
        action a {
            pt_s p;
            exec pre_solve { p = '{3, 4}; }
        };
    };
};
PSS

cat >"$TMP/m08a.pss" <<'PSS'
package m08a {
    enum e_t { A, B, C };
    struct s_t { bit[8] f; };
    component c {
        action a {
            bit b1; int i32; bool flag; e_t ev; s_t sv; string str;
            bit[16] arr[2]; list<int> li; set<int> st;
            exec pre_solve {
                b1 = 1; i32 = -5; flag = 1; ev = B; str = "x";
                arr = {11, 22}; sv.f = 9; li = {1}; st.push_back(2);
            }
        };
    };
};
PSS

cat >"$TMP/m08b.pss" <<'PSS'
package m08b {
    component c {
        action a { rand bool flag; };
    };
};
PSS

cat >"$TMP/m09.pss" <<'PSS'
package m09 {
    component c {
        action a {
            rand bit[8]  ok8;
            rand bit[8]  bad8;
            rand bit[64] bad64;
            rand bit[4]  bad4;
            rand bit     b1;
            constraint ok8   in [10..20];
            constraint bad8  in [256..300];
            constraint bad64 == 18446744073709551615;
            constraint bad4  in [16..20];
        };
    };
};
PSS

cat >"$TMP/m10.pss" <<'PSS'
package m10 {
    component c {
        action a {
            float32 f32;
            float64 f64;
            exec pre_solve { f32 = 20.14; f64 = 2e6; }
        };
    };
};
PSS

cat >"$TMP/m11a.pss" <<'PSS'
package m11a {
    enum color_e { RED, GREEN, BLUE };
    enum kind_e : bit[4] { RD = 0, WR = 1, IDLE = 2 };
    component c {
        action a {
            rand color_e col;
            rand kind_e  kind;
            constraint col  in { RED, GREEN };
            constraint kind in { RD, WR };
        };
    };
};
PSS

cat >"$TMP/m11b.pss" <<'PSS'
package m11b {
    enum seq_e  { A = 5, B, C };
    enum wide_e : bit[8] { W0 = 200, W1 = 201 };
    component c {
        action a {
            rand seq_e  s;
            rand wide_e w;
            constraint s in { A, C };
            constraint w in { W0, W1 };
        };
    };
};
PSS

cat >"$TMP/m12.pss" <<'PSS'
package m12 {
    component c {
        action a {
            string name;
            int len;
            exec pre_solve { name = "frame"; len = name.length(); }
        };
    };
};
PSS

cat >"$TMP/m13.pss" <<'PSS'
package m13 {
    component c {
        action a {
            chandle h;
            int self_eq;
            exec pre_solve { self_eq = (h == h); }
        };
    };
};
PSS

cat >"$TMP/m14.pss" <<'PSS'
package m14 {
    struct inner_s { bit[8] a; };
    struct outer_s { bit[4] op; inner_s in; int n; };
    struct derived_s : outer_s { bit[2] extra; };
    component c {
        action a {
            rand outer_s   o;
            rand derived_s d;
            int m_flat; int m_nested; int m_own; int m_inherited;
            constraint o.op     == 9;
            constraint o.n      == 5;
            constraint o.in.a   == 77;
            constraint d.extra  == 3;
            constraint d.n      == 6;
            constraint m_flat      == o.n;
            constraint m_nested    == o.in.a;
            constraint m_own       == d.extra;
            constraint m_inherited == d.n;
        };
    };
};
PSS

cat >"$TMP/m15a.pss" <<'PSS'
package m15a {
    component c {
        action a {
            int arr[3]; list<int> li; set<int> st;
            int sz_arr; int a1; int sz_li; int l1; int sz_st; int st0; int st1;
            exec pre_solve {
                arr = {4, 5, 6};
                sz_arr = arr.size(); a1 = arr[1];
                li.push_back(10); li.push_back(20);
                sz_li = li.size(); l1 = li[1];
                st.push_back(7); st.push_back(7); st.push_back(8);
                sz_st = st.size(); st0 = st[0]; st1 = st[1];
            }
        };
    };
};
PSS

cat >"$TMP/m15b.pss" <<'PSS'
package m15b {
    component c {
        action a {
            rand bit[8] sizes[4];
            constraint foreach (sizes[i]) { sizes[i] in [0..10]; }
            constraint sizes.sum() <= 25;
        };
    };
};
PSS

cat >"$TMP/m16.pss" <<'PSS'
package m16 {
    struct data_s { int v; };
    component c {
        action a {
            data_s  real_obj;
            ref data_s alias_r;
            int after;
            exec pre_solve {
                real_obj.v = 42;
                alias_r.v  = 99;
                after = real_obj.v;
            }
        };
    };
};
PSS

cat >"$TMP/m17.pss" <<'PSS'
package m17 {
    typedef bit[12] addr_t;
    typedef int     count_t;
    struct base_s { bit[8] a; };
    typedef base_s alias_s;
    component c {
        action a {
            rand addr_t  addr;
            rand count_t cnt;
            rand alias_s al;
            constraint addr in [0..100];
            constraint cnt  in [0..5];
        };
    };
};
PSS

cat >"$TMP/m18.pss" <<'PSS'
package m18 {
    component c {
        action a {
            int wide; int narrow; int single; int noop; int neg;
            int zero; int four; int allones; int src;
            exec pre_solve {
                src = 300;
                wide    = (bit[16])70000;
                narrow  = (bit[8])300;
                single  = (bit)5;
                noop    = (int)7;
                neg     = (bit[8])(-1);
                zero    = (bit[8])256;
                four    = (bit[4])20;
                allones = (bit[16])-1;
            }
        };
    };
};
PSS

cat >"$TMP/m19.pss" <<'PSS'
package m19 {
    component c {
        action a {
            int add; int sub; int mul; int div; int mod; int pow;
            int band; int bor; int bxor; int bnot; int shl; int shr;
            int cmp_lt; int cmp_eq; int cmp_ge;
            int logi_and; int logi_or; int logi_not;
            int precedence; int paren;
            int uneg; int uminus; int negdiv; int negmod; int bigsh; int and15;
            exec pre_solve {
                add = 7 + 3; sub = 7 - 3; mul = 7 * 3; div = 7 / 2; mod = 7 % 3;
                pow = 3 ** 3; band = 12 & 10; bor = 12 | 10; bxor = 12 ^ 10;
                bnot = ~0; shl = 1 << 4; shr = 32 >> 3;
                cmp_lt = (3 < 5); cmp_eq = (3 == 4); cmp_ge = (5 >= 5);
                logi_and = (1 && 0); logi_or = (1 || 0); logi_not = !0;
                precedence = 2 + 3 * 4; paren = (2 + 3) * 4;
                uneg = ~5; uminus = -5; negdiv = -7 / 2; negmod = -7 % 3;
                bigsh = 1 << 62; and15 = 255 & 15;
            }
        };
    };
};
PSS

cat >"$TMP/m20.pss" <<'PSS'
package m20 {
    component c {
        action a {
            int t1; int t2; int t3; int f1; int z; int fromfield; int fromexpr; int nested;
            exec pre_solve {
                t1 = (1 == 1) ? 10 : 20;
                t2 = (1 == 0) ? 10 : 20;
                t3 = (3 > 2) ? (4 + 5) : 0;
                f1 = 0 ? 7 : 8;
                z = 1;
                fromfield = (z == 1) ? 11 : 22;
                fromexpr  = (2 + 2 == 4) ? 55 : 66;
                nested    = (z == 1) ? ((z == 1) ? 5 : 6) : 7;
            }
        };
    };
};
PSS

cat >"$TMP/m21.pss" <<'PSS'
package m21 {
    component c {
        action a {
            int arr[3];
            int in_hit; int in_miss; int lit_hit; int lit_miss;
            exec pre_solve {
                arr = {10, 20, 30};
                in_hit   = (20 in arr);
                in_miss  = (99 in arr);
                lit_hit  = (2 in [1, 2, 3]);
                lit_miss = (9 in [1, 2, 3]);
            }
        };
    };
};
PSS

cat >"$TMP/m22.pss" <<'PSS'
package m22 {
    component c {
        action a {
            string s; string c0; string c5; string rng; string tail; string head;
            string one; string whole;
            exec pre_solve {
                s = "abcdef";
                c0    = s[0];
                c5    = s[5];
                rng   = s[1..3];
                tail  = s[3..];
                head  = s[..2];
                one   = s[2..2];
                whole = s[0..5];
            }
        };
    };
};
PSS

cat >"$TMP/m23a.pss" <<'PSS'
package m23a {
    component comp_a {
        action act_a { int v; int w; exec pre_solve { v = 5; w = 55; } };
    };
};
PSS

cat >"$TMP/m23b.pss" <<'PSS'
package m23b {
    component comp_b {
        action act_b { int v; exec pre_solve { v = 6; } };
    };
};
PSS

cat >"$TMP/m23c.pss" <<'PSS'
package m23c {
    component comp_a {
        action act_a { int v; exec pre_solve { v = 5; } };
    };
    component comp_b {
        action act_b { int v; exec pre_solve { v = 6; } };
    };
};
PSS

cat >"$TMP/m24.pss" <<'PSS'
package m24 {
    component c {
        action act_a { int v; int w; exec pre_solve { v = 5; w = 50; } };
        action act_b { int v; exec pre_solve { v = 6; } };
    };
};
PSS

cat >"$TMP/m25a.pss" <<'PSS'
package m25a {
    struct pair_s<bit[W], int N> { rand bit[W] f; rand int g; int arr[N]; };
    component c {
        action a {
            pair_s<bit[8], 4> small;
            pair_s<bit[16], 2> big;
            exec pre_solve { small.arr = {1,2,3,4}; big.arr = {5,6}; }
        };
    };
};
PSS

cat >"$TMP/m25b.pss" <<'PSS'
package m25b {
    struct pair_s<bit[W], int N> { rand bit[W] f; rand int g; int arr[N]; };
    component c {
        action a {
            pair_s<bit[8], 4> small;
            constraint small.f == 200;
            constraint small.g == 33;
        };
    };
};
PSS

# ============================================================ the assertions

# ---- 1 Numeric constants -------------------------------------------------
# Decimal, based literals (b/o/d/h), underscore separators, power, and the
# signed forms. Satya's documented rule (docs/SEMANTICS.md, "based literals"):
# the token carries the *value* and the size is a width hint — so 4'sd12 is 12
# and 16'sH_FF is 255. The assertion pins that documented contract.
begin 1 "Numeric constants"
if run m01 a 1; then
    eq dec 42
    eq b4 10
    eq h8 255
    eq o8 15
    eq d64 2
    eq us 1000000
    eq p 1024
    eq neg -42
    eq sd 12
    eq sh 255
else
    fail "model rejected: $(err1)"
fi
end

# ---- 2 String literals ---------------------------------------------------
begin 2 "String literals"
if run m02 a 1; then
    eqs lit "hello"
    eqs copy "hello"
    eqs joined "hello!"
    eqs empty_s ""
    eqs long3 "abcdef"
    eq len 5
    eq len_empty 0
    eq len_long 6
else
    fail "model rejected: $(err1)"
fi
end

# ---- 3 Aggregate literals ------------------------------------------------
begin 3 "Aggregate literals"
if run m03 a 1; then
    eq "arr.size" 3
    eq "arr[0]" 10
    eq "arr[1]" 20
    eq "arr[2]" 30
    eq "li.size" 2
    eq "li[0]" 7
    eq "li[1]" 8
    eq r0 10
    eq r1 20
    eq r2 30
    eq l0 7
    eq l1 8
    eq lsz 2
    eq b4v 5
else
    fail "model rejected: $(err1)"
fi
end

# ---- 4 Empty aggregate literal -------------------------------------------
# `arr = {}` must empty the collection: size must fall from 3 to 0.
begin 4 "Empty aggregate literal"
if run m04 a 1; then
    eq sz_before 3
    eq sz_after 0
else
    fail "model rejected: $(err1)"
fi
end

# ---- 5 Value list literal ------------------------------------------------
# `in [1, 2, 3]` / `in [7, 11]` must confine the field to those values, over 20
# seeds, and must actually choose between them (not pin one constant).
begin 5 "Value list literal"
in_set m05 a x 1 2 3
in_set m05 a y 7 11
_xs=""
_s=1
while [ "$_s" -le $SEEDS ]; do
    _xs="$_xs $(solve_get m05 a "$_s" x)"
    _s=$((_s + 1))
done
case " $_xs " in
    *" 2 "*|*" 3 "*) : ;;
    *) fail "x never left 1 across $SEEDS seeds (observed:$_xs)" ;;
esac
end

# ---- 6 Map literal -------------------------------------------------------
# Documented "partial: maps are not solved". A 2-entry literal must give
# size 2 and a keyed read must return the mapped value; insert must add one.
begin 6 "Map literal"
if run m06 a 1; then
    _szl=$(jnum sz_lit); _got=$(jnum got); _szi=$(jnum sz_ins); _g3=$(jnum got3)
    [ "$_szl" = 2 ] || fail "m={1:10,2:20} gives size $_szl (expected 2)"
    [ "$_got" = 10 ] || fail "keyed read m[1] returned $_got (expected 10)"
    [ "$_szi" = 3 ] || fail "after insert size $_szi (expected 3)"
    [ "$_g3" = 30 ] || fail "m[3]=$_g3 (expected 30)"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-OK — 'maps are not solved' reproduces"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 7 Structure literal -------------------------------------------------
# Standard keyed form '{x : 3, y : 4} must bind p.x and p.y; the positional
# form '{3, 4} must bind the same two fields in declaration order.
begin 7 "Structure literal"
if run m07a a 1; then
    eq "p.x" 3
    eq "p.y" 4
    [ -z "$CFAIL" ] || fail "keyed form '{x : 3, y : 4}: $CFAIL"
else
    _e=$(err1)
    fail "keyed form '{x : 3, y : 4} rejected: $_e"
fi
if run m07b a 1; then
    _px=$(jnum "p.x"); _py=$(jnum "p.y"); _p0=$(jnum "p[0]"); _p1=$(jnum "p[1]")
    [ "$_px" = 3 ] && [ "$_py" = 4 ] || \
        fail "positional '{3,4} leaves p.x=$_px p.y=$_py (wrote p[0]=$_p0 p[1]=$_p1)"
else
    fail "positional form '{3,4} rejected: $(err1)"
fi
[ -z "$CFAIL" ] || fail "audit: FAIL"
end

# ---- 8 Data types and data declarations ----------------------------------
# Every declared kind must carry a value, and a `bool` must be 0 or 1 over 20
# seeds.
begin 8 "Data types and data declarations"
if run m08a a 1; then
    eq b1 1
    eq i32 -5
    eq ev 1
    eqs str "x"
    eq "arr.size" 2
    eq "arr[1]" 22
    eq "sv.f" 9
    eq "st.size" 1
else
    fail "declaration model rejected: $(err1)"
fi
in_set m08b a flag 0 1
[ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE — bool is not confined to its two values"
end

# ---- 9 Integer type declaration -----------------------------------------
# The declared width is the contract, and it must hold even when a constraint
# asks for more: a bit[8] cannot exceed 255, a bit[4] cannot exceed 15, an
# unsigned bit[64] cannot be negative, and a bare bit is 0 or 1. The `ok8`
# field shows the ordinary in-range case working, over 20 seeds.
begin 9 "Integer type declaration"
in_range m09 a ok8 10 20
in_range m09 a bad8 0 255
in_range m09 a bad64 0 18446744073709551615
in_range m09 a bad4 0 15
in_set m09 a b1 0 1
[ -z "$CFAIL" ] || fail "declared width not enforced; audit: PARTIAL-WORSE"
end

# ---- 10 Floating-point type declaration ---------------------------------
# f32 must hold an IEEE-754 binary32 pattern for 20.14 (= 1101078200, and so
# < 2^32); f64 must hold the binary64 pattern for 2e6.
begin 10 "Floating-point type declaration"
if run m10 a 1; then
    _f32=$(jnum f32); _f64=$(jnum f64)
    if [ "$_f32" != 1101078200 ]; then
        if [ "$_f32" -ge 4294967296 ] 2>/dev/null; then
            fail "f32=20.14 stored as 64-bit pattern $_f32 (binary32 is 1101078200): float32 is 8 bytes"
        else
            fail "f32=20.14 stored as $_f32 (binary32 is 1101078200)"
        fi
    fi
    [ "$_f64" = 4701340746312056832 ] || fail "f64=2e6 stored as $_f64 (expected 4701340746312056832)"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE — float32 does not narrow"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 11 enum declaration -------------------------------------------------
# Two things must hold: a field constrained to a subset of the enumerators
# takes only those, and an enumerator with an explicit initializer carries the
# value it is declared with (`W1 = 201` is 201, not its position).
begin 11 "enum declaration"
in_set m11a a col 0 1
in_set m11a a kind 0 1
if run m11b a 1; then
    _w=$(jnum w); _s=$(jnum s)
    case $_w in
        200|201) : ;;
        *) fail "enum wide_e : bit[8] { W0 = 200, W1 = 201 } gives w=$_w (declared value ignored; position used)" ;;
    esac
    case $_s in
        5|7) : ;;
        *) fail "enum seq_e { A = 5, B, C } gives s=$_s under 'in { A, C }' (A=5, C=7)" ;;
    esac
else
    fail "explicit-initializer model rejected: $(err1)"
fi
[ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE"
end

# ---- 12 string declaration ----------------------------------------------
begin 12 "string declaration"
if run m12 a 1; then
    eqs name "frame"
    eq len 5
else
    fail "model rejected: $(err1)"
fi
end

# ---- 13 chandle declaration ---------------------------------------------
# Documented "partial: opaque handle". A handle still compares equal to itself.
begin 13 "chandle declaration"
if run m13 a 1; then
    _e=$(jnum self_eq)
    [ "$_e" = 1 ] || fail "h == h evaluates to $_e (expected 1); handle carries no value"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-OK — 'opaque handle' reproduces"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 14 struct declaration ----------------------------------------------
# Flat, nested, inherited and own members are each constrained to a value and
# read back through a second field, so binding is observed rather than inferred
# from the trace's field list.
begin 14 "struct declaration"
if run m14 a 1; then
    eq m_flat 5
    eq m_own 3
    eq m_nested 77
    _inh=$(jnum m_inherited)
    [ "$_inh" = 6 ] || fail "inherited member d.n constrained to 6 binds nothing (read back $_inh)"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 15 Collection data types -------------------------------------------
# Array/list access and size, set uniqueness (7 pushed twice must give size 2),
# and foreach-bound element constraints over 20 seeds.
begin 15 "Collection data types"
if run m15a a 1; then
    eq "arr.size" 3
    eq a1 5
    eq "li.size" 2
    eq l1 20
    eq sz_st 2
    eq "st.size" 2
    [ -z "$CFAIL" ] || fail "set<int> kept a duplicate: 7 pushed twice, 8 once"
else
    fail "collection model rejected: $(err1)"
fi
# foreach over the array: every element in [0..10] and the sum <= 25, per seed.
_s=1
while [ "$_s" -le $SEEDS ]; do
    if run m15b a "$_s"; then
        _sum=0; _i=0; _ok=1
        while [ "$_i" -lt 4 ]; do
            _v=$(jnum "sizes[$_i]")
            case $_v in __ABSENT__|__ERROR__|"") _ok=0; break ;; esac
            if [ "$_v" -lt 0 ] || [ "$_v" -gt 10 ]; then _ok=0; break; fi
            _sum=$((_sum + _v)); _i=$((_i + 1))
        done
        if [ "$_ok" -eq 0 ]; then fail "foreach: sizes[$_i]=$_v outside [0..10] at seed $_s"; break; fi
        if [ "$_sum" -gt 25 ]; then fail "foreach: element sum $_sum > 25 at seed $_s"; break; fi
    else
        fail "foreach model rejected at seed $_s: $(err1)"; break
    fi
    _s=$((_s + 1))
done
[ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE"
end

# ---- 16 ref declaration -------------------------------------------------
# Documented "partial: reference semantics not modelled". A write through the
# ref must be visible in the referent.
begin 16 "ref declaration"
if run m16 a 1; then
    _after=$(jnum after)
    [ "$_after" = 99 ] || fail "write through ref alias_r.v=99 left real_obj.v=$_after (write did not propagate)"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-OK — documented limitation reproduces"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 17 User-defined type declaration -----------------------------------
# A typedef name must resolve to its underlying type: the bit[12] typedef
# confines to [0..100], the int typedef to [0..5], the struct typedef exposes
# its member. Over 20 seeds.
begin 17 "User-defined type declaration"
in_range m17 a addr 0 100
in_range m17 a cnt 0 5
if run m17 a 1; then
    _a=$(jnum "al.a")
    [ "$_a" = "__ABSENT__" ] && fail "typedef alias_s of struct base_s exposes no member al.a"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 18 cast operation --------------------------------------------------
begin 18 "cast operation"
if run m18 a 1; then
    eq wide 4464
    eq narrow 44
    eq single 1
    eq noop 7
    eq neg 255
    eq zero 0
    eq four 4
    eq allones 65535
else
    fail "model rejected: $(err1)"
fi
end

# ---- 19 Expressions and operators ---------------------------------------
begin 19 "Expressions and operators"
if run m19 a 1; then
    eq add 10;      eq sub 4;      eq mul 21;   eq div 3;    eq mod 1
    eq pow 27;      eq band 8;     eq bor 14;   eq bxor 6;   eq bnot -1
    eq shl 16;      eq shr 4;      eq cmp_lt 1; eq cmp_eq 0; eq cmp_ge 1
    eq logi_and 0;  eq logi_or 1;  eq logi_not 1
    eq precedence 14; eq paren 20
    eq uneg -6;     eq uminus -5;  eq negdiv -3; eq negmod -1
    eq bigsh 4611686018427387904; eq and15 15
else
    fail "model rejected: $(err1)"
fi
end

# ---- 20 Conditional operator --------------------------------------------
begin 20 "Conditional operator"
if run m20 a 1; then
    eq t1 10; eq t2 20; eq t3 9; eq f1 8
    eq fromfield 11; eq fromexpr 55; eq nested 5
else
    fail "model rejected: $(err1)"
fi
end

# ---- 21 Set membership operator -----------------------------------------
# The operator must return a true boolean both over a named collection and
# over a literal value list: (20 in arr) is 1, (99 in arr) is 0, and likewise
# for [1, 2, 3].
begin 21 "Set membership operator"
if run m21 a 1; then
    eq in_hit 1
    eq in_miss 0
    eq lit_hit 1
    eq lit_miss 0
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE — membership over a literal list is not evaluated"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 22 String slice operator -------------------------------------------
begin 22 "String slice operator"
if run m22 a 1; then
    eqs c0 "a"
    eqs c5 "f"
    eqs rng "bcd"
    eqs tail "def"
    eqs head "abc"
    eqs one "c"
    eqs whole "abcdef"
else
    fail "model rejected: $(err1)"
fi
end

# ---- 23 component declaration -------------------------------------------
# A component type is declared, owns its actions and its fields, and a step is
# reported under the component that declares the action it ran. Each component
# is probed in its own model (a package with two top-level components and no
# explicit top is ambiguous — Satya picks one per seed — so that model is used
# only for the weaker claim that both components coexist and each action keeps
# its own body).
begin 23 "component declaration"
if run m23a act_a 1; then
    _c=$(jget component); _v=$(jnum v); _w=$(jnum w)
    [ "$_c" = '"comp_a"' ] || fail "act_a reported under component $_c (expected comp_a)"
    [ "$_v" = 5 ] || fail "comp_a.act_a.v=$_v (expected 5)"
    [ "$_w" = 55 ] || fail "comp_a.act_a.w=$_w (expected 55)"
else
    fail "comp_a model rejected: $(err1)"
fi
if run m23b act_b 1; then
    _c=$(jget component); _v=$(jnum v)
    [ "$_c" = '"comp_b"' ] || fail "act_b reported under component $_c (expected comp_b)"
    [ "$_v" = 6 ] || fail "comp_b.act_b.v=$_v (expected 6)"
else
    fail "comp_b model rejected: $(err1)"
fi
_va=$(solve_get m23c act_a 1 v)
_vb=$(solve_get m23c act_b 1 v)
[ "$_va" = 5 ] || fail "two components: act_a.v=$_va (expected 5)"
[ "$_vb" = 6 ] || fail "two components: act_b.v=$_vb (expected 6)"
end

# ---- 24 action declaration ----------------------------------------------
# Two actions in one component: each solves independently and owns its fields.
begin 24 "action declaration"
if run m24 act_a 1; then
    eq v 5
    eq w 50
else
    fail "act_a rejected: $(err1)"
fi
if run m24 act_b 1; then
    eq v 6
    _w=$(jnum w)
    [ "$_w" = "__ABSENT__" ] || fail "act_b carries act_a's field w=$_w"
else
    fail "act_b rejected: $(err1)"
fi
end

# ---- 25 Template type declaration ---------------------------------------
# Two probes. (a) each instantiation must be its own type: pair_s<bit[8],4>
# gives a 4-element array (index 3 exists), pair_s<bit[16],2> a 2-element one
# (index 2 does not). (b) the instantiated type's fields must bind under a
# constraint like any other type's.
begin 25 "Template type declaration"
if run m25a a 1; then
    _s3=$(jnum "small.arr[3]")
    [ "$_s3" = 4 ] || fail "pair_s<bit[8],4>.arr[3]=$_s3 (expected 4): N=4 not substituted"
    _b2=$(jnum "big.arr[2]")
    [ "$_b2" = "__ABSENT__" ] || fail "pair_s<bit[16],2>.arr[2]=$_b2: N=2 not substituted"
    [ -z "$CFAIL" ] || fail "instantiation widths wrong"
else
    fail "instantiation model rejected: $(err1)"
fi
if run m25b a 1; then
    _f=$(jnum "small.f"); _g=$(jnum "small.g")
    [ "$_f" = 200 ] || fail "rand templated field small.f=$_f under 'small.f == 200' (no field of the instantiation is reported)"
    [ "$_g" = 33 ]  || fail "rand templated field small.g=$_g under 'small.g == 33' (bound nothing)"
    [ -z "$CFAIL" ] || fail "audit: PARTIAL-WORSE — rand templated fields bind nothing while the counter reports 2/2 satisfied"
else
    fail "constrained template model rejected: $(err1)"
fi
end

# ------------------------------------------------------------------- verdict
if [ "$GLOBAL_FAIL" -eq 0 ]; then
    exit 0
fi
exit 1
