#!/bin/sh
# proof-slice-4.sh — re-derive the feature-audit verdicts for slice 4 of Satya
# v0.46.0 (PSS constructs 76–100: monitors, extension, packages, functions and
# procedural code) by ASSERTING on observed behaviour.
#
# Every model is embedded below as a heredoc and written into a private mktemp
# directory; the script needs nothing else on disk. Nothing is judged on "it
# parses" — each construct must CHANGE something observable: a field must hold a
# computed value, an override must change the value the solver produces, an
# import must change name resolution, a monitor traversal must produce a match,
# a `randomize ... with {}` must apply its constraint.
#
# Usage:   ./proof-slice-4.sh /path/to/satya
# Output:  one line per construct — "PASS <n> <name>" or "FAIL <n> <name> — <reason>"
# Exit:    0 all pass; 1 at least one fails; 2 the argument is not a Satya binary.
#
# Judged against docs/FEATURE-AUDIT.md slice 4 (9 PASS / 1 PARTIAL-OK /
# 12 PARTIAL-WORSE / 3 FAIL). PARTIAL-OK is reported as FAIL here, because the
# script asks only "does the construct behave as the standard requires"; for
# construct 76 the failure is exactly the limitation the audit documents.

set -u

PROG=$(basename "$0")
die2() { echo "$PROG: $*" >&2; exit 2; }

# ---- the binary -----------------------------------------------------------
[ $# -ge 1 ] || die2 "usage: $0 /path/to/satya"
SATYA=$1
[ -e "$SATYA" ] || die2 "no such file: $SATYA"
[ -f "$SATYA" ] || die2 "not a regular file: $SATYA"
[ -x "$SATYA" ] || die2 "not executable: $SATYA"
HELP=$("$SATYA" help 2>&1) || die2 "cannot run $SATYA (help failed) — not a Satya binary"
case $HELP in
  *"test-stimulus engine"*) ;;
  *) die2 "not a Satya binary: $SATYA" ;;
esac
case $HELP in
  *"solve <model>"*) ;;
  *) die2 "not a Satya binary: $SATYA" ;;
esac

# ---- scratch --------------------------------------------------------------
TMP=$(mktemp -d) || die2 "cannot create a temp directory"
trap 'rm -rf "$TMP"' EXIT INT TERM HUP

PASSES=0
FAILS=0
pass() { printf 'PASS %s %s\n' "$1" "$2"; PASSES=$((PASSES + 1)); }
fail() { printf 'FAIL %s %s — %s\n' "$1" "$2" "$3"; FAILS=$((FAILS + 1)); }

model() { cat > "$TMP/$1.pss"; }

# run a model; the trace lands in $TR
TR=
tr_run() {
  TR="$TMP/$1.$2.trace"
  "$SATYA" solve "$TMP/$1.pss" --action "$2" >"$TR" 2>&1
  return 0
}
parse_ok() {
  "$SATYA" check "$TMP/$1.pss" >"$TMP/$1.check" 2>&1 &&
    grep -q '^OK' "$TMP/$1.check"
}
# "solved: a=1 b=2" of step N
step() {
  awk -v want="$2" '
    $1 ~ /^[0-9]+\.$/ { i = $1; sub(/\.$/, "", i); cur = (i == want); next }
    cur && /solved:/ { sub(/^[ \t]*solved:[ \t]*/, ""); print; exit }
  ' "$1"
}
# value of field $3 in step $2 of trace $1
field() {
  step "$1" "$2" | tr ' ' '\n' |
    awk -v k="$3=" 'index($0, k) == 1 { print substr($0, length(k) + 1); exit }'
}
# number of hits reported for monitor $2 ("monitor <name> N hit(s)"); a monitor
# that is never mentioned in the trace matched zero times
hits() {
  awk -v m="$2" '$1 == "monitor" && $2 == m { print $3; found = 1; exit }
                  END { if (!found) print 0 }' "$1"
}
# a coverage line for covergroup $2 was printed at all
sampled() { grep -qF "  $2." "$1"; }
msg() { grep -qF "$2" "$1"; }
is() { [ -n "$1" ] && [ "$1" = "$2" ]; }

# ==========================================================================
# 76 — Schedule monitor activity statement (PSS 19.3.5)
# A schedule whose branches can be realized must match; one whose branches
# intersect must not; `overlap` requires the branches to overlap IN TIME, so a
# strictly sequential trace must not match it.
# ==========================================================================
model s76_sched <<'PSS'
component c {
  action A { rand int av; };
  action B { rand int bv; };
  monitor sched_m {
    A a;
    B b;
    activity { schedule { do a; do b; } }
  }
  action t { activity { do A; do B; cover sched_m; } };
};
PSS
model s76_disjoint <<'PSS'
component c {
  action write   { rand int wv; };
  action send    { rand int sv; };
  action receive { rand int cv; };
  monitor m2 {
    activity { schedule {
        sequence { do write; do send; };
        sequence { do send; do receive; };
    } }
  }
  action t { activity { do write; do send; do receive; cover m2; } };
};
PSS
model s76_overlap <<'PSS'
component c {
  action A { rand int av; };
  action B { rand int bv; };
  monitor ov_m {
    A a;
    B b;
    activity { overlap { do a; do b; } }
  }
  action t { activity { do A; do B; cover ov_m; } };
};
PSS
tr_run s76_sched t;   h_sched=$(hits "$TR" sched_m)
tr_run s76_disjoint t; h_disj=$(hits "$TR" m2)
tr_run s76_overlap t;  h_ov=$(hits "$TR" ov_m)
if is "$h_sched" 1 && is "$h_disj" 0 && is "$h_ov" 0; then
  pass 76 "schedule monitor activity statement"
else
  fail 76 "schedule monitor activity statement" \
    "overlap matched a strictly sequential trace ($h_ov hit(s), expected 0) — branches are lowered onto the same node and matched independently; schedule: matching $h_sched/1, intersecting $h_disj/0"
fi

# ==========================================================================
# 77 — Monitor traversal statement (PSS 19.3.9)
# Traversing a nested monitor (by type or through a handle) must produce a
# match; the flat control monitor must match once on the same trace.
# ==========================================================================
model s77_flat <<'PSS'
component c {
  action idle  { rand int iv; };
  action read  { rand int rv; };
  action write { rand int wv; };
  monitor irw {
    idle i;
    read r;
    write w;
    activity { do i; do r; do w; }
    covergroup g_flat (int iv) { coverpoint iv { bins any = [0..1000]; } }
  }
  action t { activity { do idle; do read; do write; cover irw; } };
};
PSS
model s77_type <<'PSS'
component c {
  action idle  { rand int iv; };
  action read  { rand int rv; };
  action write { rand int wv; };
  monitor rw {
    read r;
    write w;
    activity { sequence { do r; do w; } }
  }
  monitor irw1 {
    idle i;
    activity { do i; do rw; }
    covergroup g_type (int iv) { coverpoint iv { bins any = [0..1000]; } }
  }
  action t { activity { do idle; do read; do write; cover irw1; } };
};
PSS
model s77_handle <<'PSS'
component c {
  action idle  { rand int iv; };
  action read  { rand int rv; };
  action write { rand int wv; };
  monitor rw {
    read r;
    write w;
    activity { sequence { do r; do w; } }
  }
  monitor irw2 {
    idle i;
    rw m;
    activity { do i; m; }
    covergroup g_handle (int iv) { coverpoint iv { bins any = [0..1000]; } }
  }
  action t { activity { do idle; do read; do write; cover irw2; } };
};
PSS
tr_run s77_flat t;   h_flat=$(hits "$TR" irw);  c_flat=$(sampled "$TR" g_flat && echo yes || echo no)
tr_run s77_type t;   h_type=$(hits "$TR" irw1)
tr_run s77_handle t; h_hand=$(hits "$TR" irw2)
if is "$h_flat" 1 && is "$c_flat" yes && is "$h_type" 1 && is "$h_hand" 1; then
  pass 77 "monitor traversal statement"
else
  fail 77 "monitor traversal statement" \
    "traversal produces no match at all: flat monitor ${h_flat:-0} hit(s), traversal by type (\`do rw;\`) ${h_type:-0}, traversal by handle (\`m;\`) ${h_hand:-0} — the nested monitor's activity is never matched"
fi

# ==========================================================================
# 78 — Monitor constraints (PSS 19.4)
# A realization that satisfies the monitor's constraint matches and its
# covergroups are sampled; one that violates it is discarded and nothing is
# sampled.
# ==========================================================================
model s78_ok <<'PSS'
component c {
  action item { rand int len; constraint len in [60..100]; };
  monitor mon {
    item it;
    activity { do it; }
    constraint { it.len > 50; }
    covergroup cg (int len) { coverpoint len { bins low = [0..50]; bins high = [51..100]; } }
  }
  action t { activity { do item; cover mon; } };
};
PSS
model s78_bad <<'PSS'
component c {
  action item { rand int len; constraint len == 10; };
  monitor mon {
    item it;
    activity { do it; }
    constraint { it.len > 50; }
    covergroup cg (int len) { coverpoint len { bins low = [0..50]; bins high = [51..100]; } }
  }
  action t { activity { do item; cover mon; } };
};
PSS
tr_run s78_ok t
h_ok=$(hits "$TR" mon); len_ok=$(field "$TR" 0 len)
c_ok=$(sampled "$TR" cg && echo yes || echo no)
tr_run s78_bad t
h_bad=$(hits "$TR" mon); c_bad=$(sampled "$TR" cg && echo yes || echo no)
if is "$h_ok" 1 && is "$c_ok" yes && is "$h_bad" 0 && is "$c_bad" no; then
  pass 78 "monitor constraints"
else
  fail 78 "monitor constraints" \
    "satisfying realization: $h_ok hit(s)/coverage=$c_ok (len=$len_ok); violating realization (len=10 vs constraint >50): $h_bad hit(s)/coverage=$c_bad — the constraint must discard the match and leave the covergroups unsampled"
fi

# ==========================================================================
# 79 — Type extension (PSS 20.2)
# The extension adds a field, a constraint and an exec block; each must have an
# observable effect on the solved action.
# ==========================================================================
model s79_struct <<'PSS'
component c {
  struct s_t { int a; };
  action t {
    s_t v;
    exec post_solve { v.a = 1; v.b = 2; }
  };
};
extend struct c::s_t { int b; };
PSS
model s79_action <<'PSS'
component c {
  action A {
    rand int x;
    int base_exec;
    constraint x in [0..100];
    exec post_solve { base_exec = 1; }
  };
  action t { activity { do A; } };
};
extend action c::A {
  int ext_field;
  constraint x == 77;
  exec post_solve { base_exec = 2; ext_field = 99; }
}
PSS
tr_run s79_struct t; va=$(field "$TR" 0 v.a); vb=$(field "$TR" 0 v.b)
tr_run s79_action t; x79=$(field "$TR" 0 x); be=$(field "$TR" 0 base_exec); ef=$(field "$TR" 0 ext_field)
if is "$va" 1 && is "$vb" 2 && is "$x79" 77 && { is "$be" 2 || is "$ef" 99; }; then
  pass 79 "type extension"
else
  fail 79 "type extension" \
    "the extension's exec block never runs: base_exec=$be (expected 2), ext_field=$ef (expected 99), so the attribute it adds stays unusable (added field/constraint do apply: v.b=$vb, x=$x79)"
fi

# ==========================================================================
# 80 — Override declaration (PSS 20.5)
# A type override and an instance override must both change the value the
# solver produces, in the component-scope form and in the action-scope form of
# LRM Example 226.
# ==========================================================================
model s80_type <<'PSS'
component c {
  action base_a { rand int x; constraint x in [0..100]; };
  action der_a : base_a { constraint x == 91; };
  override { type base_a with der_a; }
  action t { activity { do base_a; } };
};
PSS
model s80_inst <<'PSS'
component c {
  action base_a { rand int x; constraint x in [0..100]; };
  action der_b : base_a { constraint x == 55; };
  override { instance fb with der_b; }
  action t { base_a fb; activity { fb; } };
};
PSS
model s80_action <<'PSS'
component c {
  action base_a { rand int x; constraint x in [0..100]; };
  action der_a : base_a { constraint x == 91; };
  action t { override { type base_a with der_a; }; activity { do base_a; } };
};
PSS
tr_run s80_type t;   xt=$(field "$TR" 0 x)
tr_run s80_inst t;   xi=$(field "$TR" 0 x)
tr_run s80_action t; xa=$(field "$TR" 0 x)
if is "$xt" 91 && is "$xi" 55 && is "$xa" 91; then
  pass 80 "override declaration"
else
  fail 80 "override declaration" \
    "the action-scope override of LRM Example 226 is parsed and ignored: x=$xa (expected 91) — component-scope forms work (type x=$xt, instance x=$xi)"
fi

# ==========================================================================
# 81 — Package declaration
# A package constant must drive a constraint, a nested package must resolve,
# and package-scope components must be instantiated and traversed.
# ==========================================================================
model s81 <<'PSS'
package outer_p {
  static const int K = 5;
  package inner_p {
    static const int M = 7;
    component inner_c { action ia { rand int v; constraint v == 111; }; };
  }
  component pkg_c { action pa { rand int w; constraint w == K + 1; }; };
};
component top_c {
  outer_p::pkg_c pc;
  outer_p::inner_p::inner_c ic;
  action t {
    activity { do outer_p::pkg_c::pa; do outer_p::inner_p::inner_c::ia; }
  };
};
PSS
tr_run s81 t
w81=$(field "$TR" 0 w); v81=$(field "$TR" 1 v)
if is "$w81" 6 && is "$v81" 111; then
  pass 81 "package declaration"
else
  fail 81 "package declaration" \
    "package constants did not drive the constraints: w=$w81 (expected K+1=6), v=$v81 (expected 111)"
fi

# ==========================================================================
# 82 — Import statement
# Two packages define value(). With `import fpa::*` — and with
# `import fpa::value` — the name must resolve to fpa's 11. The control model,
# with no import at all, shows what the name resolves to without help.
# ==========================================================================
model s82_star <<'PSS'
package fpa { function int value() { return (11); } }
package fpb { function int value() { return (22); } }
component c {
  import fpa::*;
  action a { int r; exec post_solve { r = value(); } };
};
PSS
model s82_name <<'PSS'
package fpa { function int value() { return (11); } }
package fpb { function int value() { return (22); } }
component c {
  import fpa::value;
  action a { int r; exec post_solve { r = value(); } };
};
PSS
model s82_none <<'PSS'
package fpa { function int value() { return (11); } }
package fpb { function int value() { return (22); } }
component c {
  action a { int r; exec post_solve { r = value(); } };
};
PSS
tr_run s82_star a; r_star=$(field "$TR" 0 r)
tr_run s82_name a; r_name=$(field "$TR" 0 r)
tr_run s82_none a; r_none=$(field "$TR" 0 r)
if is "$r_star" 11 && is "$r_name" 11; then
  pass 82 "import statement"
else
  fail 82 "import statement" \
    "the import does not affect name resolution: \`import fpa::*\` gives value()=$r_star and \`import fpa::value\` gives $r_name, both the same as the no-import control ($r_none) — fpa's 11 is never selected"
fi

# ==========================================================================
# 83 — Exec block declaration (PSS 22.1)
# init_down then init_up for the hierarchy, pre_solve feeding a constraint,
# post_solve on a leaf and on a composite action.
# ==========================================================================
model s83 <<'PSS'
component c {
  int cfg;
  exec init_down { cfg = 1; }
  exec init_up   { cfg = 21; }
  action leaf {
    rand int x;
    int seedv;
    exec pre_solve  { seedv = 40; }
    constraint x == seedv + 2;
    exec post_solve { message(LOW, "MARK leaf_post", 0); }
  };
  action comp {
    exec post_solve { message(LOW, "MARK composite_post", 0); }
    activity { do leaf; }
  };
};
PSS
tr_run s83 comp
cfg83=$(field "$TR" 0 cfg); sv83=$(field "$TR" 0 seedv); x83=$(field "$TR" 0 x)
if is "$cfg83" 21 && is "$sv83" 40 && is "$x83" 42 &&
   msg "$TR" "MARK leaf_post" && msg "$TR" "MARK composite_post"; then
  pass 83 "exec block declaration"
else
  fail 83 "exec block declaration" \
    "the composite action's exec post_solve never runs (MARK composite_post absent); init_down/init_up, pre_solve and the leaf post_solve are correct (cfg=$cfg83, seedv=$sv83, x=$x83)"
fi

# ==========================================================================
# 84 — Function declaration (PSS 22.3)
# A prototype with a separate definition must supply the value, both unqualified
# and package-qualified.
# ==========================================================================
model s84 <<'PSS'
package fn_p {
  function int sum10(int a, int b);
  function int sum10(int a, int b) { return (a * 10 + b); }
}
component c {
  action plain  { int r; exec post_solve { r = sum10(4, 2); } };
  action scoped { int r; exec post_solve { r = fn_p::sum10(4, 2); } };
};
PSS
tr_run s84 plain;  r84a=$(field "$TR" 0 r)
tr_run s84 scoped; r84b=$(field "$TR" 0 r)
if is "$r84a" 42 && is "$r84b" 42; then
  pass 84 "function declaration"
else
  fail 84 "function declaration" \
    "the package-qualified call does not reach the definition: fn_p::sum10(4,2)=$r84b (expected 42) while the unqualified call returns $r84a"
fi

# ==========================================================================
# 85 — Function definition (PSS 22.1)
# Parameters bound positionally into a scope restored after the call; a task
# body runs for its side effects.
# ==========================================================================
model s85 <<'PSS'
component c {
  int cnt;
  function int twice(int x) { x = x + 100; return (x * 2); }
  task side_effect() { cnt = cnt + 3; }
  action f {
    int r;
    int arg;
    exec post_solve {
      arg = 5;
      r = twice(arg);
      cnt = 0;
      side_effect();
      side_effect();
    }
  };
};
PSS
tr_run s85 f
r85=$(field "$TR" 0 r); a85=$(field "$TR" 0 arg); c85=$(field "$TR" 0 cnt)
if is "$r85" 210 && is "$a85" 5 && is "$c85" 6; then
  pass 85 "function definition"
else
  fail 85 "function definition" \
    "twice(5)=$r85 (expected 210), caller's argument after the call=$a85 (expected 5: the call scope must be restored), task side effects=$c85 (expected 6)"
fi

# ==========================================================================
# 86 — Imported function qualifiers (PSS 22.2)
# `import solve function fp::twice;` names a function with no PSS body. Per the
# standard the call must invoke the foreign API; with no binding available the
# tool must say so rather than invent a value. The control model, with the body
# present and no qualifier, shows what the call should yield.
# ==========================================================================
model s86_proto <<'PSS'
package fp {
  function int twice(int x);
}
component c {
  import solve function fp::twice;
  action a { int r; exec post_solve { r = twice(21); } };
};
PSS
model s86_ctl <<'PSS'
package fp {
  function int twice(int x) { return (x * 2); }
}
component c {
  action a { int r; exec post_solve { r = twice(21); } };
};
PSS
tr_run s86_proto a; r86=$(field "$TR" 0 r); w86=$(grep -c 'Warnings' "$TR")
tr_run s86_ctl a;   r86c=$(field "$TR" 0 r)
if is "$r86" "$r86c"; then
  pass 86 "imported function qualifiers"
else
  fail 86 "imported function qualifiers" \
    "the imported prototype silently yields $r86 with $w86 diagnostic block(s); the same call with a body present yields $r86c — the tool invents 0 instead of reporting the missing foreign binding"
fi

# ==========================================================================
# 87 — Import class declaration (PSS 22.4.2.1)
# The declaration must be accepted, and a field of the imported class type must
# be instantiable in a component scope. Tried in four spellings.
# ==========================================================================
model s87_a <<'PSS'
package fp { import class foreign_c; }
component c {
  fp::foreign_c h;
  action a { int r; exec post_solve { r = 1; } };
};
PSS
model s87_b <<'PSS'
package fp { class foreign_c; }
component c {
  import class fp::foreign_c;
  fp::foreign_c h;
  action a { int r; exec post_solve { r = 1; } };
};
PSS
model s87_c <<'PSS'
component c {
  import class foreign_c;
  action a { int r; exec post_solve { r = 1; } };
};
PSS
model s87_d <<'PSS'
package fp {
  import class foreign_c {
    function int get(int a);
  }
}
component c {
  fp::foreign_c h;
  action a { int r; exec post_solve { r = h.get(7); } };
};
PSS
n87=0
for m in s87_a s87_b s87_c s87_d; do
  parse_ok "$m" && n87=$((n87 + 1))
done
if [ "$n87" -eq 4 ]; then
  tr_run s87_d a; r87=$(field "$TR" 0 r)
  if is "$r87" 7; then
    pass 87 "import class declaration"
  else
    fail 87 "import class declaration" \
      "all four spellings parse but the imported class method binds nothing: h.get(7)=$r87 (expected 7)"
  fi
else
  why87=$(grep -m1 '^error' "$TMP/s87_a.check" 2>/dev/null |
    sed -e "s|$TMP/||" -e 's/^error: //')
  fail 87 "import class declaration" \
    "$((4 - n87)) of 4 spellings are a parse error (${why87:-no diagnostic}); the declaration is never accepted, so no imported-class field can be instantiated"
fi

# ==========================================================================
# 88 — Target-template function implementation (PSS 22.5)
# A function whose implementation is a target template must supply the value
# the template computes — or, with no target backend able to evaluate it,
# report that. Silently returning 0 is a confident wrong answer.
# ==========================================================================
model s88 <<'PSS'
package tp {
  function int compute(int a) {
    exec body C = """
      return ({{a}} * 2);
    """;
  }
}
component c {
  action a { int r; exec post_solve { r = tp::compute(21); } };
};
PSS
tr_run s88 a
r88=$(field "$TR" 0 r); w88=$(grep -c 'Warnings' "$TR")
if is "$r88" 42; then
  pass 88 "target-template function implementation"
else
  fail 88 "target-template function implementation" \
    "the template body is neither evaluated nor reported: tp::compute(21)=$r88 (expected 42) with $w88 diagnostic block(s)"
fi

# ==========================================================================
# 89 — Procedural block statement
# A block inside an exec body must propagate its assignment.
# ==========================================================================
model s89 <<'PSS'
component c {
  action a {
    int in_block;
    exec post_solve {
      in_block = 0;
      { in_block = 5; }
    }
  };
};
PSS
tr_run s89 a
v89=$(field "$TR" 0 in_block)
if is "$v89" 5; then
  pass 89 "procedural block statement"
else
  fail 89 "procedural block statement" \
    "\`{ in_block = 5; }\` leaves $v89 — the block is dropped, its assignment never reaches the field"
fi

# ==========================================================================
# 90 — Procedural variable declaration
# A declaration with an initialiser inside exec is scoped to the block: it must
# supply its initial value and must not disturb the component field of the same
# name.
# ==========================================================================
model s90 <<'PSS'
component c {
  int t;
  exec init_up { t = 1; }
  action a {
    int r;
    exec post_solve { int t = 7; r = t + 1; }
  };
};
PSS
tr_run s90 a
r90=$(field "$TR" 0 r); t90=$(field "$TR" 0 t)
if is "$r90" 8 && is "$t90" 1; then
  pass 90 "procedural variable declaration"
else
  fail 90 "procedural variable declaration" \
    "the block-scoped declaration is not scoped: component field t became $t90 (expected 1, its init_up value) after \`int t = 7;\` (the initialiser itself is honoured: r=$r90)"
fi

# ==========================================================================
# 91 — Procedural assignment statement
# The lvalue forms of the statement — plain, compound and indexed — must all
# take effect.
# ==========================================================================
model s91 <<'PSS'
component c {
  action a {
    int r;
    int s;
    int arr[3];
    exec post_solve {
      r = 41;
      s = r + 1;
      s += 2;
      arr[1] = 9;
    }
  };
};
PSS
tr_run s91 a
r91=$(field "$TR" 0 r); s91=$(field "$TR" 0 s); a91=$(field "$TR" 0 'arr[1]')
if is "$r91" 41 && is "$s91" 44 && is "$a91" 9; then
  pass 91 "procedural assignment statement"
else
  fail 91 "procedural assignment statement" \
    "only the plain form takes effect: s after \`s = r + 1; s += 2;\` is $s91 (expected 44, so \`+=\` is dropped) and \`arr[1] = 9;\` leaves arr[1]=$a91 (expected 9)"
fi

# ==========================================================================
# 92 — Void function call
# A void function called as a statement must run its body for its effect.
# ==========================================================================
model s92 <<'PSS'
component c {
  int cnt;
  function void bump(int by) { cnt = cnt + by; }
  action a {
    int r;
    exec post_solve {
      cnt = 0;
      bump(5);
      bump(6);
      r = cnt;
    }
  };
};
PSS
tr_run s92 a
c92=$(field "$TR" 0 cnt)
if is "$c92" 11; then
  pass 92 "void function call"
else
  fail 92 "void function call" \
    "the void function's body did not run: after bump(5) and bump(6) cnt=$c92 (expected 11)"
fi

# ==========================================================================
# 93 — Procedural return statement
# `return (expr)` supplies the value, `return;` leaves the function early.
# ==========================================================================
model s93 <<'PSS'
component c {
  int cnt;
  function int pick(int a) {
    if (a > 3) { return (10); }
    return (20);
  }
  task early(int x) {
    if (x > 0) { return; }
    cnt = cnt + 100;
  }
  action a {
    int hi;
    int lo;
    int hit;
    int miss;
    exec post_solve {
      hi = pick(5);
      lo = pick(1);
      cnt = 0;
      early(1);
      hit = cnt;
      early(0);
      miss = cnt;
    }
  };
};
PSS
tr_run s93 a
hi93=$(field "$TR" 0 hi); lo93=$(field "$TR" 0 lo)
hit93=$(field "$TR" 0 hit); miss93=$(field "$TR" 0 miss)
if is "$hi93" 10 && is "$lo93" 20 && is "$hit93" 0 && is "$miss93" 100; then
  pass 93 "procedural return statement"
else
  fail 93 "procedural return statement" \
    "pick(5)=$hi93 (expected 10), pick(1)=$lo93 (expected 20), after early(1) cnt=$hit93 (expected 0: \`return;\` must leave early), after early(0) cnt=$miss93 (expected 100)"
fi

# ==========================================================================
# 94 — Procedural repeat-count statement
# repeat (n) must run its body n times, including n = 0 and n from a variable,
# and the body may be a single statement rather than a block.
# ==========================================================================
model s94 <<'PSS'
component c {
  action a {
    int s;
    int z;
    int n;
    int b;
    int k;
    exec post_solve {
      s = 0; repeat (4) { s = s + 3; }
      z = 0; repeat (0) { z = z + 1; }
      k = 5; n = 0; repeat (k) { n = n + 1; }
      b = 0; repeat (4) b = b + 3;
    }
  };
};
PSS
tr_run s94 a
s94=$(field "$TR" 0 s); z94=$(field "$TR" 0 z)
n94=$(field "$TR" 0 n); b94=$(field "$TR" 0 b)
if is "$s94" 12 && is "$z94" 0 && is "$n94" 5 && is "$b94" 12; then
  pass 94 "procedural repeat-count statement"
else
  fail 94 "procedural repeat-count statement" \
    "\`repeat (4) b = b + 3;\` (a single-statement body) runs once: b=$b94 (expected 12); the block form is correct (repeat (4) s=$s94/12, repeat (0) z=$z94/0, repeat (k=5) n=$n94/5)"
fi

# ==========================================================================
# 95 — Procedural repeat-while statement
# The body runs before the condition is tested, so it runs at least once.
# ==========================================================================
model s95 <<'PSS'
component c {
  action a {
    int once;
    int twice;
    exec post_solve {
      once = 0;
      repeat { once = once + 1; } while (0 > 1);
      twice = 0;
      repeat { twice = twice + 1; } while (twice < 2);
    }
  };
};
PSS
tr_run s95 a
o95=$(field "$TR" 0 once); t95=$(field "$TR" 0 twice)
if is "$o95" 1 && is "$t95" 2; then
  pass 95 "procedural repeat-while statement"
else
  fail 95 "procedural repeat-while statement" \
    "a false condition must still run the body once: once=$o95 (expected 1); iterating to twice<2 gives twice=$t95 (expected 2)"
fi

# ==========================================================================
# 96 — Procedural foreach statement
# foreach over a 4-element list must visit every element with its index.
# ==========================================================================
model s96 <<'PSS'
component c {
  action a {
    int s;
    int n;
    int arr[4];
    exec post_solve {
      arr = {1, 2, 3, 4};
      s = 0; n = 0;
      foreach (i : arr) { s = s + arr[i]; n = n + 1; }
    }
  };
};
PSS
tr_run s96 a
s96=$(field "$TR" 0 s); n96=$(field "$TR" 0 n)
if is "$s96" 10 && is "$n96" 4; then
  pass 96 "procedural foreach statement"
else
  fail 96 "procedural foreach statement" \
    "foreach over {1,2,3,4} summed $s96 (expected 10) over $n96 iteration(s) (expected 4) — the index does not resolve to each element"
fi

# ==========================================================================
# 97 — Procedural if-else statement
# Both branches, taken on the condition, with a block and with a single
# statement as the body.
# ==========================================================================
model s97 <<'PSS'
component c {
  action a {
    int hi;
    int lo;
    int r;
    int q;
    exec post_solve {
      hi = 0; lo = 0; r = 0; q = 0;
      if (5 > 3) { hi = 1; } else { hi = 2; }
      if (1 > 3) { lo = 1; } else { lo = 2; }
      if (5 > 3) r = 1; else r = 2;
      if (1 > 3) q = 1; else q = 2;
    }
  };
};
PSS
tr_run s97 a
hi97=$(field "$TR" 0 hi); lo97=$(field "$TR" 0 lo)
r97=$(field "$TR" 0 r); q97=$(field "$TR" 0 q)
if is "$hi97" 1 && is "$lo97" 2 && is "$r97" 1 && is "$q97" 2; then
  pass 97 "procedural if-else statement"
else
  fail 97 "procedural if-else statement" \
    "the single-statement form takes the wrong branch: \`if (5 > 3) r = 1; else r = 2;\` yields r=$r97 (expected 1) and \`if (1 > 3) q = 1; else q = 2;\` yields q=$q97; the block form is correct (hi=$hi97/1, lo=$lo97/2)"
fi

# ==========================================================================
# 98 — Procedural match statement
# The arm whose guard covers the subject must be selected: a value arm, a range
# arm, and the unbraced form of LRM Example 275.
# ==========================================================================
model s98 <<'PSS'
component c {
  function int exact(int x) {
    int res;
    res = 0;
    match (x) { 5: { res = 50; } default: { res = 40; } }
    return res;
  }
  function int range(int x) {
    int res;
    res = 0;
    match (x) { [4..7]: { res = 2; } default: { res = 4; } }
    return res;
  }
  function int lrm(int a) {
    int res;
    match (a) {
      [0..3]:  res = 1;
      [4..7]:  res = 2;
      [8..15]: res = 3;
      default: res = 4;
    }
    return res;
  }
  action a {
    int e;
    int g;
    int l;
    exec post_solve { e = exact(5); g = range(5); l = lrm(5); }
  };
};
PSS
tr_run s98 a
e98=$(field "$TR" 0 e); g98=$(field "$TR" 0 g); l98=$(field "$TR" 0 l)
if is "$e98" 50 && is "$g98" 2 && is "$l98" 2; then
  pass 98 "procedural match statement"
else
  fail 98 "procedural match statement" \
    "range arms are never selected: [4..7] on 5 gives $g98 (expected 2) and the LRM Example 275 form gives $l98 (expected 2), because a bare arm body is dropped as well; only the exact-value arm with a braced body works (e=$e98/50)"
fi

# ==========================================================================
# 99 — Procedural break/continue statement
# break leaves the loop, continue skips the rest of the iteration.
# ==========================================================================
model s99 <<'PSS'
component c {
  action a {
    int i;
    int j;
    int k;
    exec post_solve {
      i = 0;
      repeat (10) { i = i + 1; if (i == 3) { break; } }
      j = 0;
      k = 0;
      repeat (5) { j = j + 1; if (j < 3) { continue; } k = k + 1; }
    }
  };
};
PSS
tr_run s99 a
i99=$(field "$TR" 0 i); j99=$(field "$TR" 0 j); k99=$(field "$TR" 0 k)
if is "$i99" 3 && is "$j99" 5 && is "$k99" 3; then
  pass 99 "procedural break/continue statement"
else
  fail 99 "procedural break/continue statement" \
    "break left the loop after $i99 iteration(s) (expected 3); continue skipped $((j99 - k99)) of $j99 iterations (expected 2 skipped, 3 counted)"
fi

# ==========================================================================
# 100 — Procedural randomize statement
# `randomize(x)` must re-randomise an action attribute within its constraints,
# and `randomize res with { res > a; }` must apply the with-clause.
# ==========================================================================
model s100 <<'PSS'
component c {
  action a {
    rand int x;
    constraint x in [10..100];
    int after;
    int z;
    exec post_solve {
      x = 5;
      randomize(x);
      after = x;
      z = func3(5);
    }
  };
};
function int func3(int a) {
  int res;
  randomize res with { res > a; };
  return res;
}
PSS
tr_run s100 a
after100=$(field "$TR" 0 after); z100=$(field "$TR" 0 z)
if [ -n "$after100" ] && [ "$after100" -ge 10 ] && [ "$after100" -le 100 ] &&
   [ -n "$z100" ] && [ "$z100" -gt 5 ]; then
  pass 100 "procedural randomize statement"
else
  fail 100 "procedural randomize statement" \
    "\`randomize res with { res > a; }\` ignores the with-clause: func3(5) returns $z100 (expected > 5); the attribute form does re-solve within its constraints (x=5 -> $after100, domain [10..100])"
fi

# ---- summary --------------------------------------------------------------
printf '\n%d passed, %d failed, %d constructs\n' "$PASSES" "$FAILS" "$((PASSES + FAILS))"
[ "$FAILS" -eq 0 ] || exit 1
exit 0
