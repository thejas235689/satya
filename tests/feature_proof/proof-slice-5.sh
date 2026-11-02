#!/usr/bin/env bash
# proof-slice-5.sh — assertion-based re-derivation of the slice-5 verdicts
# (PSS constructs 101–125: compile-time constructs, executors, address space)
# of the Satya v0.46.0 feature audit (docs/FEATURE-AUDIT.md).
#
# Usage:   ./proof-slice-5.sh /path/to/satya
# Output:  one line per construct — "PASS <n> <name>" or "FAIL <n> <name> — <reason>"
# Exit:    0 if every construct passes, 1 if any fails, 2 if $1 is not a Satya binary.
#
# Assertion policy: every model is embedded below as a heredoc into a mktemp
# directory; nothing else on disk is needed.  Each verdict is derived from
# observed behaviour only — exit status, solved values in the trace, messages,
# warnings and file side effects.  Nothing is inferred from the parser's own
# report.  A construct is asserted at the strength the project documents for it:
# rows documented `yes` are asserted to work, rows documented `partial` are
# asserted to reproduce exactly their documented limitation and not to be worse
# than it.  So PASS here == audit PASS or PARTIAL-OK, and FAIL == PARTIAL-WORSE
# or FAIL.

set -u

BIN=${1:-}
if [ -z "$BIN" ]; then
    echo "usage: $0 /path/to/satya" >&2
    exit 2
fi
if [ ! -e "$BIN" ]; then
    echo "error: $BIN does not exist" >&2
    exit 2
fi
if [ ! -f "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "error: $BIN is not an executable file" >&2
    exit 2
fi
case "$BIN" in
    /*) ;;
    *) BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")" ;;
esac
if ! "$BIN" help 2>&1 | grep -q "Satya"; then
    echo "error: $BIN does not look like a Satya binary (\`help\` printed no Satya banner)" >&2
    exit 2
fi

W=$(mktemp -d "${TMPDIR:-/tmp}/proof-slice-5.XXXXXX") || exit 2
trap 'rm -rf "$W"' EXIT
cd "$W" || exit 2

PASSES=0
FAILS=0
pass() { printf 'PASS %s %s\n' "$1" "$2"; PASSES=$((PASSES + 1)); }
fail() {
    printf 'FAIL %s %s — %s\n' "$1" "$2" "$(printf '%s' "$3" | sed 's/[; ]*$//')"
    FAILS=$((FAILS + 1))
}

# ---------------------------------------------------------------- helpers ---
OUT=""
RC=0
run() { # run [args...] — sets OUT (stdout+stderr) and RC
    OUT=$("$BIN" "$@" 2>&1)
    RC=$?
}

# solved_val <output> <key> — value of `key` on the trace's `solved:` line
solved_val() {
    printf '%s\n' "$1" | awk -v k="$2" '
        /solved:/ {
            n = split($0, p, " ")
            for (i = 1; i <= n; i++) {
                if (index(p[i], k "=") == 1) { print substr(p[i], length(k) + 2); exit }
            }
        }'
}

has() { printf '%s\n' "$1" | grep -q -- "$2"; }

# eq <got> <want> <label> — echoes a complaint, or nothing
eq() { [ "$1" = "$2" ] || printf '%s expected %s, got %s; ' "$3" "$2" "${1:-<absent>}"; }

# ================================================================== 101 =====
cat > m101.pss <<'EOF'
component c {
    action test {
        int a; int b;
        exec pre_solve { a = 1; yield(); b = 2; }
        rand int v; constraint v == 5;
    };
};
EOF
run solve m101.pss
R=""
R="$R$(eq "$RC" 0 'exit')"
R="$R$(eq "$(solved_val "$OUT" a)" 1 'a')"
R="$R$(eq "$(solved_val "$OUT" b)" 2 'b after yield')"
if [ -z "$R" ]; then pass 101 "Procedural yield statement"
else fail 101 "Procedural yield statement" "exec body does not survive yield(): $R"; fi

# ================================================================== 102 =====
# An export declaration must make `pkg::A1` nameable.  Differential: the same
# model with and without the export statement.  With the export, `do pkg::A1`
# must resolve; without it, it must not.
cat > m102_exp.pss <<'EOF'
package pkg { export target c::A1(bit mode); }
component c {
    action A1 { rand bit mode; rand bit[31:0] val; };
    action test { activity { do pkg::A1; } rand int v; constraint v == 3; };
};
EOF
cat > m102_noexp.pss <<'EOF'
package pkg { }
component c {
    action A1 { rand bit mode; rand bit[31:0] val; };
    action test { activity { do pkg::A1; } rand int v; constraint v == 3; };
};
EOF
run solve m102_exp.pss
WITH=$RC
run solve m102_noexp.pss
WITHOUT=$RC
if [ "$WITH" = 0 ] && [ "$WITHOUT" != 0 ]; then
    pass 102 "Export action declaration"
else
    if [ "$WITHOUT" = 0 ]; then
        fail 102 "Export action declaration" \
            "the export statement has no effect: pkg::A1 resolves (exit 0) with no export declaration at all"
    else
        fail 102 "Export action declaration" \
            "the exported name pkg::A1 does not resolve even with the export declaration (exit $WITH)"
    fi
fi

# ================================================================== 103 =====
# compile if: the selected branch is kept, the other is not, and no
# neighbouring declaration is lost — at component scope, nested, in an action
# body and at package scope.
cat > m103_top.pss <<'EOF'
component c {
    action before_a { rand int v; constraint v == 1; };
    compile if (0) {
        action dead_a { rand int v; };
    } else {
        action live_a { rand int v; constraint v == 2; };
    }
    action after_a { rand int v; constraint v == 3; };
};
EOF
cat > m103_nest.pss <<'EOF'
component c {
    compile if (1) {
        action plain_a { rand int v; constraint v == 1; };
        compile if (1) {
            action inner_a { rand int v; constraint v == 2; };
        }
    }
    action after_a { rand int v; constraint v == 3; };
};
EOF
cat > m103_body.pss <<'EOF'
component c {
    action test {
        compile if (0) {
            rand int gone;
        } else {
            rand int kept;
        }
        rand int v; constraint v == 1;
    };
};
EOF
cat > m103_pkg.pss <<'EOF'
package p {
    compile if (0) {
        static const A = 11;
    } else {
        static const B = 22;
    }
}
component c {
    action test {
        int a; int b;
        exec pre_solve { b = p::B; }
        rand int v; constraint v == 1;
    };
};
EOF
R=""
run solve --action after_a m103_top.pss
[ "$RC" = 0 ] || R="$R top-level: neighbouring action after_a lost;"
run solve --action live_a m103_top.pss
[ "$RC" = 0 ] || R="$R top-level: selected branch live_a not instantiated;"
run solve --action dead_a m103_top.pss
[ "$RC" != 0 ] || R="$R top-level: unselected branch dead_a was instantiated;"
run solve --action inner_a m103_nest.pss
[ "$RC" = 0 ] || R="$R nested: declaration inner_a from the enabled nested branch lost;"
run solve --action test m103_body.pss
[ "$RC" = 0 ] || R="$R action body: the action no longer solves;"
if [ "$(solved_val "$OUT" kept)" != 0 ]; then
    R="$R action body: declaration kept from the enabled branch was lost (and its neighbour v with it);"
fi
run solve --action test m103_pkg.pss
B=$(solved_val "$OUT" b)
[ "$B" = 22 ] || R="$R package scope: p::B from the enabled branch is not 22 (got ${B:-<absent>});"
if [ -z "$R" ]; then pass 103 "compile if declaration"
else fail 103 "compile if declaration" "$(printf '%s' "$R" | sed 's/^ //; s/;  */; /g')"; fi

# ================================================================== 104 =====
# compile has: true for a symbol that is declared, false for one that is
# declared nowhere.
cat > m104_yes.pss <<'EOF'
component c { }
component d {
    compile if (compile has (c)) {
        action then_a { rand int v; constraint v == 7; };
    } else {
        action else_a { rand int v; constraint v == 8; };
    }
};
EOF
cat > m104_no.pss <<'EOF'
component d {
    compile if (compile has (NO_SUCH_TYPE_ANYWHERE)) {
        action then_a { rand int v; constraint v == 7; };
    } else {
        action else_a { rand int v; constraint v == 8; };
    }
};
EOF
R=""
run solve --action then_a m104_yes.pss
[ "$RC" = 0 ] || R="$R declared symbol c: the then-branch was not taken;"
run solve --action else_a m104_no.pss
[ "$RC" = 0 ] || R="$R undeclared symbol: the else-branch was not taken (compile has() is true for a name declared nowhere);"
if [ -z "$R" ]; then pass 104 "compile has expression"
else fail 104 "compile has expression" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 105 =====
cat > m105_bad.pss <<'EOF'
component c {
    compile assert (1 + 1 == 3);
    action test { rand int v; constraint v == 1; };
};
EOF
cat > m105_good.pss <<'EOF'
component c {
    compile assert (1 + 1 == 2);
    action test { rand int v; constraint v == 1; };
};
EOF
R=""
run check m105_bad.pss
[ "$RC" != 0 ] || R="$R a false compile assert was not an error;"
has "$OUT" "1 + 1 == 3" || R="$R the error does not name the failing expression;"
run solve m105_good.pss
[ "$RC" = 0 ] || R="$R a true compile assert was rejected;"
[ "$(solved_val "$OUT" v)" = 1 ] || R="$R the declaration after a true compile assert was lost;"
if [ -z "$R" ]; then pass 105 "compile assert statement"
else fail 105 "compile assert statement" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 106 =====
cat > m106.pss <<'EOF'
component c {
    action test {
        string msg;
        int n;
        exec pre_solve { n = 42; msg = format("v=%d h=%x s=%s p=%%", n, 255, "hi"); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m106.pss
R=""
has "$OUT" 'msg = "v=42 h=ff s=hi p=%"' || R="$R format() did not produce v=42 h=ff s=hi p=%"
if [ -z "$R" ]; then pass 106 "String formatting and output functions"
else fail 106 "String formatting and output functions" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 107 =====
cat > m107.pss <<'EOF'
component c {
    action test {
        exec pre_solve { print("hello"); message("note"); error("boom"); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m107.pss
R=""
has "$OUT" "INFO: hello" || R="$R print() emitted no INFO message;"
has "$OUT" "INFO: note" || R="$R message() emitted no message;"
has "$OUT" "ERROR: boom" || R="$R error() emitted no ERROR message;"
if [ -z "$R" ]; then pass 107 "Runtime messaging function"
else fail 107 "Runtime messaging function" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 108 =====
# Documented partial: accepted, and no effect on the scenario.  Assert exactly
# that — accepted without error, scenario unaffected — and that the documented
# limitation still reproduces (no file appears).
F108="$W/fh_slice5.txt"
rm -f "$F108"
cat > m108.pss <<EOF
component c {
    action test {
        file fh;
        int rc;
        exec pre_solve { fh = fopen("$F108", "w"); rc = fwrite(fh, "hello"); rc = fclose(fh); }
        rand int v; constraint v == 2;
    };
};
EOF
run solve m108.pss
R=""
[ "$RC" = 0 ] || R="$R fopen/fwrite/fclose were rejected;"
[ "$(solved_val "$OUT" v)" = 2 ] || R="$R the scenario was disturbed by the file operations;"
[ ! -e "$F108" ] || R="$R a file was written (the audit records the construct as having no effect)"
if [ -z "$R" ]; then pass 108 "Text file operations using file handles"
else fail 108 "Text file operations using file handles" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 109 =====
F109="$W/fprintf_slice5.txt"
rm -f "$F109"
cat > m109.pss <<EOF
component c {
    action test {
        int rc;
        exec pre_solve { rc = fprintf("$F109", "x=%d\n", 7); }
        rand int v; constraint v == 2;
    };
};
EOF
run solve m109.pss
R=""
[ "$RC" = 0 ] || R="$R fprintf() was rejected;"
[ "$(solved_val "$OUT" v)" = 2 ] || R="$R the scenario was disturbed by fprintf;"
[ ! -e "$F109" ] || R="$R a file was written (the audit records the construct as having no effect)"
if [ -z "$R" ]; then pass 109 "Simple text file operations"
else fail 109 "Simple text file operations" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 110 =====
# error() reports and execution continues; fatal() reports a fatal error and
# terminates the run, so the statement after it must not execute.
cat > m110.pss <<'EOF'
component c {
    action test {
        int a; int b;
        exec pre_solve { a = 1; error("boom"); fatal("dead"); b = 2; }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m110.pss
R=""
has "$OUT" "ERROR: boom" || R="$R error() did not report an ERROR message;"
has "$OUT" "FATAL: dead" || R="$R fatal() did not report a FATAL message;"
[ "$(solved_val "$OUT" a)" = 1 ] || R="$R execution did not reach fatal();"
B=$(solved_val "$OUT" b)
if [ "$RC" = 0 ] && [ "$B" = 2 ]; then
    R="$R fatal() did not terminate the run: the next statement still executed (b=2) and the run exited 0;"
fi
if [ -z "$R" ]; then pass 110 "Error reporting functions"
else fail 110 "Error reporting functions" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 111 =====
cat > m111.pss <<'EOF'
component c {
    action test {
        int u1; int u2; int r;
        exec pre_solve { u1 = urandom(); u2 = urandom(); r = urandom_range(1, 6); }
        rand int v; constraint v == 1;
    };
};
EOF
R=""
SEEN=""
BAD=""
for s in 1 2 3 4 5 6 7 8; do
    run solve --seed "$s" m111.pss
    r=$(solved_val "$OUT" r)
    u1=$(solved_val "$OUT" u1)
    u2=$(solved_val "$OUT" u2)
    if [ -z "$r" ]; then BAD="yes"; continue; fi
    if [ "$r" -lt 1 ] || [ "$r" -gt 6 ]; then BAD="yes"; r_out="$r"; fi
    [ "$u1" != "$u2" ] || BAD="yes"
    SEEN="$SEEN $r"
done
DISTINCT=$(printf '%s\n' $SEEN | sort -u | wc -l | tr -d ' ')
[ -z "$BAD" ] || R="$R urandom_range(1,6) left its range or urandom() repeated a value${r_out:+ (r=$r_out)};"
[ "$DISTINCT" -gt 1 ] || R="$R urandom_range(1,6) returned a constant ($SEEN)"
if [ -z "$R" ]; then pass 111 "Randomization functions"
else fail 111 "Randomization functions" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 112 =====
# 6.0 as float64 is 0x4018000000000000 = 4618441417868443648; as float32 it
# must be 0x40C00000 = 1086324736.
cat > m112.pss <<'EOF'
component c {
    action test {
        float32 f32; float64 f64;
        exec pre_solve { f32 = 6.0; f64 = 6.0; }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m112.pss
R=""
R="$R$(eq "$(solved_val "$OUT" f64)" 4618441417868443648 'float64 6.0')"
R="$R$(eq "$(solved_val "$OUT" f32)" 1086324736 'float32 6.0')"
if [ -z "$R" ]; then pass 112 "Floating-point storage types"
else fail 112 "Floating-point storage types" "$R"; fi

# ================================================================== 113 =====
cat > m113.pss <<'EOF'
component c {
    action test {
        int m6; int m1;
        exec pre_solve { m6 = float_mantissa(6.0); m1 = float_mantissa(1.0); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m113.pss
R=""
R="$R$(eq "$(solved_val "$OUT" m6)" 2251799813685248 'float_mantissa(6.0)')"
R="$R$(eq "$(solved_val "$OUT" m1)" 0 'float_mantissa(1.0)')"
if [ -z "$R" ]; then pass 113 "float_mantissa function"
else fail 113 "float_mantissa function" "$R"; fi

# ================================================================== 114 =====
cat > m114.pss <<'EOF'
component c {
    action test {
        int e6; int e1; int e2;
        exec pre_solve { e6 = float_exponent(6.0); e1 = float_exponent(1.0); e2 = float_exponent(2.0); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m114.pss
R=""
R="$R$(eq "$(solved_val "$OUT" e6)" 1025 'float_exponent(6.0)')"
R="$R$(eq "$(solved_val "$OUT" e1)" 1023 'float_exponent(1.0)')"
R="$R$(eq "$(solved_val "$OUT" e2)" 1024 'float_exponent(2.0)')"
if [ -z "$R" ]; then pass 114 "float_exponent function"
else fail 114 "float_exponent function" "$R"; fi

# ================================================================== 115 =====
cat > m115.pss <<'EOF'
component c {
    action test {
        int s6; int sneg;
        exec pre_solve { s6 = float_sign(6.0); sneg = float_sign(-6.0); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m115.pss
R=""
R="$R$(eq "$(solved_val "$OUT" s6)" 0 'float_sign(6.0)')"
R="$R$(eq "$(solved_val "$OUT" sneg)" 1 'float_sign(-6.0)')"
if [ -z "$R" ]; then pass 115 "float_sign function"
else fail 115 "float_sign function" "$R"; fi

# ================================================================== 116 =====
# to_float(mantissa, exponent, sign): 1.0 must be 0x3FF0000000000000 and
# -1.0 must be 0xBFF0000000000000 = -4616189618054758400.
cat > m116.pss <<'EOF'
component c {
    action test {
        float64 p; float64 n;
        exec pre_solve { p = to_float(0, 1023, 0); n = to_float(0, 1023, 1); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m116.pss
R=""
R="$R$(eq "$(solved_val "$OUT" p)" 4607182418800017408 'to_float(0,1023,0) = 1.0')"
R="$R$(eq "$(solved_val "$OUT" n)" -4616189618054758400 'to_float(0,1023,1) = -1.0')"
if [ -z "$R" ]; then pass 116 "to_float function"
else fail 116 "to_float function" "$R"; fi

# ================================================================== 117 =====
# Executor component: an executor_c instance is a component with a trait
# struct whose values are settable and readable.
cat > m117.pss <<'EOF'
struct t_s : executor_trait_s { rand int core; };
component c {
    executor_c<t_s> e0;
    executor_c<t_s> e1;
    exec init_down { e0.trait.core = 0; e1.trait.core = 1; }
    action test {
        int c0; int c1;
        exec pre_solve { c0 = e0.trait.core; c1 = e1.trait.core; }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m117.pss
R=""
R="$R$(eq "$RC" 0 'exit')"
R="$R$(eq "$(solved_val "$OUT" c0)" 0 'e0.trait.core')"
R="$R$(eq "$(solved_val "$OUT" c1)" 1 'e1.trait.core')"
if [ -z "$R" ]; then pass 117 "Executor component"
else fail 117 "Executor component" "$R"; fi

# ================================================================== 118 =====
# Executor group component: executors are registered with add_executor and the
# group accepts them without error.
cat > m118.pss <<'EOF'
struct t_s : executor_trait_s { rand int core; };
component c {
    executor_c<t_s> e0;
    executor_c<t_s> e1;
    executor_group_c<t_s> eg;
    exec init_down {
        e0.trait.core = 0;
        e1.trait.core = 1;
        eg.add_executor(e0);
        eg.add_executor(e1);
    }
    action test {
        int c0; int c1;
        exec pre_solve { c0 = e0.trait.core; c1 = e1.trait.core; }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m118.pss
R=""
R="$R$(eq "$RC" 0 'exit')"
printf '%s\n' "$OUT" | grep -Eq "warnings[[:space:]]*:[[:space:]]*0" || R="$R add_executor produced a warning;"
R="$R$(eq "$(solved_val "$OUT" c1)" 1 'e1.trait.core')"
if [ -z "$R" ]; then pass 118 "Executor group component"
else fail 118 "Executor group component" "$R"; fi

# ================================================================== 119 =====
# Executor claim struct: an action's executor claim must be matched against the
# executors in its context — it must not be routed into address allocation.
cat > m119.pss <<'EOF'
struct t_s : executor_trait_s { rand int core; };
component c {
    executor_c<t_s> e0;
    executor_c<t_s> e1;
    executor_group_c<t_s> eg;
    exec init_down {
        e0.trait.core = 0;
        e1.trait.core = 1;
        eg.add_executor(e0);
        eg.add_executor(e1);
    }
    action test {
        rand executor_claim_s<t_s> ec;
        constraint ec.trait.core == 1;
        rand int v; constraint v == 1;
    };
};
EOF
run solve m119.pss
R=""
R="$R$(eq "$RC" 0 'exit')"
if has "$OUT" "could not be satisfied"; then
    R="$R the executor claim was routed into address allocation and reported unsatisfiable: $(printf '%s\n' "$OUT" | grep -m1 'could not be satisfied' | sed 's/^ *- *//')"
fi
if [ -z "$R" ]; then pass 119 "Executor claim struct"
else fail 119 "Executor claim struct" "$(printf '%s' "$R" | sed 's/^ //')"; fi

# ================================================================== 120 =====
# Executor query function: executor() must be callable, and per the standard it
# returns null in a pre_solve block (assignment is resolved only during solve).
cat > m120.pss <<'EOF'
component c {
    executor_c<> e0;
    action test {
        int h;
        exec pre_solve { h = executor(); }
        rand int v; constraint v == 1;
    };
};
EOF
run solve m120.pss
R=""
R="$R$(eq "$RC" 0 'exit')"
R="$R$(eq "$(solved_val "$OUT" h)" 0 'executor() in pre_solve must be null')"
if [ -z "$R" ]; then pass 120 "Executor query function"
else fail 120 "Executor query function" "$R"; fi

# ================================================================== 121 =====
# Generic address space: regions are modelled and claims allocate from them,
# and a claim's `size` constraint must be honoured — two 100-byte claims must
# be 100 bytes apart.
cat > m121.pss <<'EOF'
package p121 {
    struct mem_t : addr_trait_s { rand int kind; };
    component c {
        addr_space_c<mem_t> r0;
        exec init_up { r0.addr = 0x1000; r0.size = 0x1000; r0.trait.kind = 0; }
        action test {
            transparent_addr_claim_s<mem_t> ca;
            transparent_addr_claim_s<mem_t> cb;
            constraint ca.size == 100;
            constraint cb.size == 100;
            constraint ca.trait.kind == 0;
        };
    };
};
EOF
run solve --action test m121.pss
A=$(solved_val "$OUT" "ca.addr")
B=$(solved_val "$OUT" "cb.addr")
S=$(solved_val "$OUT" "ca.size")
R=""
R="$R$(eq "$A" 4096 'claim address (region 0x1000, trait kind=0 matched)')"
R="$R$(eq "$S" 100 'claim.size')"
if [ -n "$A" ] && [ -n "$B" ]; then
    R="$R$(eq "$((B - A))" 100 'gap between two 100-byte claims')"
else
    R="$R the second claim was not allocated;"
fi
if [ -z "$R" ]; then pass 121 "Generic address space component"
else fail 121 "Generic address space component" "$R"; fi

# ================================================================== 122 =====
# Contiguous address space: successive claims are laid out contiguously from the
# region base, without overlap or gaps.
cat > m122.pss <<'EOF'
component c {
    contiguous_addr_space_c<> mem;
    exec init_up { mem.addr = 0x1000; mem.size = 0x1000; }
    action test {
        transparent_addr_claim_s<> ca;
        transparent_addr_claim_s<> cb;
        transparent_addr_claim_s<> cc;
    };
};
EOF
run solve m122.pss
A=$(solved_val "$OUT" "ca.addr")
B=$(solved_val "$OUT" "cb.addr")
C=$(solved_val "$OUT" "cc.addr")
R=""
R="$R$(eq "$A" 4096 'first claim at the region base')"
R="$R$(eq "$B" 4160 'second claim immediately after the first (64 bytes)')"
R="$R$(eq "$C" 4224 'third claim immediately after the second')"
if [ -z "$R" ]; then pass 122 "Contiguous address space component"
else fail 122 "Contiguous address space component" "$R"; fi

# ================================================================== 123 =====
# Transparent address space: a region added with add_region() is where claims
# land.
cat > m123.pss <<'EOF'
component c {
    transparent_addr_space_c<> mem;
    exec init_up {
        addr_region_s<> r;
        r.addr = 0x8000;
        r.size = 512;
        (void)mem.add_region(r);
    }
    action test { transparent_addr_claim_s<> ca; };
};
EOF
run solve m123.pss
R=""
R="$R$(eq "$(solved_val "$OUT" "ca.addr")" 32768 'claim address (region base 0x8000)')"
R="$R$(eq "$(solved_val "$OUT" "ca.size")" 64 'claim size')"
if [ -z "$R" ]; then pass 123 "Transparent address space component"
else fail 123 "Transparent address space component" "$R"; fi

# ================================================================== 124 =====
# Base address region type: a region's size bounds its allocations — a 100-byte
# region cannot hold two 64-byte claims, and the second must be refused.
cat > m124.pss <<'EOF'
component c {
    addr_region_base_s<> r0;
    exec init_up { r0.addr = 0x2000; r0.size = 100; }
    action test {
        transparent_addr_claim_s<> ca;
        transparent_addr_claim_s<> cb;
    };
};
EOF
run solve m124.pss
R=""
R="$R$(eq "$(solved_val "$OUT" "ca.addr")" 8192 'first claim at the region base')"
has "$OUT" "could not be satisfied" || R="$R a second 64-byte claim was allocated from a 100-byte region"
if [ -z "$R" ]; then pass 124 "Base address region type"
else fail 124 "Base address region type" "$R"; fi

# ================================================================== 125 =====
# Contiguous address space region type: regions carry an address and a size,
# and a claim is placed in the first region that can hold it.
cat > m125.pss <<'EOF'
component c {
    contiguous_addr_space_c<> mem;
    exec init_up {
        contiguous_addr_region_s<> small;
        small.addr = 0x1000;
        small.size = 10;
        contiguous_addr_region_s<> big;
        big.addr = 0x3000;
        big.size = 1000;
        (void)mem.add_region(small);
        (void)mem.add_region(big);
    }
    action test { transparent_addr_claim_s<> ca; };
};
EOF
run solve m125.pss
R=""
R="$R$(eq "$(solved_val "$OUT" "ca.addr")" 12288 'claim address (first region that fits is the 0x3000 one)')"
if [ -z "$R" ]; then pass 125 "Contiguous address space region type"
else fail 125 "Contiguous address space region type" "$R"; fi

# ============================================================== summary =====
echo
echo "slice 5: $PASSES passed, $FAILS failed of $((PASSES + FAILS))"
if [ "$FAILS" -ne 0 ]; then exit 1; fi
if [ "$PASSES" -ne 25 ]; then
    echo "error: expected 25 verdicts, produced $((PASSES + FAILS))" >&2
    exit 1
fi
exit 0
