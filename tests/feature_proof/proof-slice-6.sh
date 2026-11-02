#!/usr/bin/env bash
# proof-slice-6.sh — assertion-based proof for Satya v0.46.0, slice 6.
#
#   ./proof-slice-6.sh /path/to/satya
#
# Part A  (A126–A146) constructs 126–146: address handles, transparent regions,
#         claims, packed structs, sizeof_s, get_tag, the byte/struct memory
#         primitives and the register constructs. Each assertion is on an
#         OBSERVED VALUE (solved fields), never on "it parses".
# Part B  (B1–B16) the CLI and the six backends: exit statuses, line-numbered
#         diagnostics, seed reproducibility and output structure.
#
# Every model is embedded below as a heredoc into a private mktemp directory.
# Nothing else on disk is needed and no network access is used.
#
# Output: one line per item, `PASS <n> <name>` or `FAIL <n> <name> — <reason>`.
# Exit 0 if every item passes, 1 if any fails, 2 on a bad binary argument.
set -u

# ---------------------------------------------------------------- binary check
if [ "$#" -lt 1 ] || [ -z "${1:-}" ]; then
    echo "usage: $0 /path/to/satya" >&2
    echo "error: no binary given" >&2
    exit 2
fi
BIN=$1
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
if ! ident=$("$BIN" help 2>&1); then
    echo "error: not Satya: \`$BIN help\` failed" >&2
    exit 2
fi
case $ident in
    *Satya*) : ;;
    *) echo "error: not Satya: \`$BIN help\` did not identify itself" >&2; exit 2 ;;
esac

TMP=$(mktemp -d "${TMPDIR:-/tmp}/proof-slice-6.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT INT TERM

PASSED=0
FAILED=0
OUT=""
RC=0
MIS=""

pass_item() { printf 'PASS %s %s\n' "$1" "$2"; PASSED=$((PASSED + 1)); }
fail_item() { printf 'FAIL %s %s — %s\n' "$1" "$2" "$3"; FAILED=$((FAILED + 1)); }

# ------------------------------------------------------------------- observers
# Value of `name` from the trace's "solved:" line.
getv() {
    awk -v k="$1" '
        /solved:/ {
            for (i = 2; i <= NF; i++) {
                n = split($i, a, "=")
                if (n == 2 && a[1] == k) { print a[2]; exit }
            }
        }' "$2"
}

# Value of the string variable `name` from the trace's "strings:" line.
getstr() {
    sed -n "s/.*strings:.*[ ]$1 = \"\([^\"]*\)\".*/\1/p" "$2" | head -1
}

# Run `solve --action a` on a model; OUT holds combined output, RC the status.
solve_model() {
    OUT=$("$BIN" solve "$1" --action a 2>&1)
    RC=$?
}

# Compare a solved field with an expected decimal value; record mismatches.
want() {
    got=$(getv "$1" "$TMP/out.txt")
    if [ -z "$got" ]; then
        MIS="$MIS $1=<absent>(want $2)"
    elif [ "$got" != "$2" ]; then
        MIS="$MIS $1=$got(want $2)"
    fi
}

wantstr() {
    got=$(getstr "$1" "$TMP/out.txt")
    if [ "$got" != "$2" ]; then
        MIS="$MIS $1=\"$got\"(want \"$2\")"
    fi
}

wantnz() {
    got=$(getv "$1" "$TMP/out.txt")
    case ${got:-} in
        ''|0) MIS="$MIS $1=${got:-<absent>}(want non-zero)" ;;
    esac
}

firstline() { printf '%s' "$1" | head -1 | cut -c1-160; }

# Solve a model and run a list of assertions on it.
#   judge ID NAME MODEL -- <assertion commands...>   is awkward in sh, so each
#   item below calls solve_model then the want* helpers then verdict.
verdict() { # ID NAME
    if [ -z "$MIS" ]; then
        pass_item "$1" "$2"
    else
        fail_item "$1" "$2" "$(printf '%s' "$MIS" | sed 's/^ //')"
    fi
    MIS=""
}

# =========================================================== embedded models
cat >"$TMP/a126.pss" <<'PSS'
// 126 — transparent region type (24.7.1.4)
component C {
  transparent_addr_space_c<> sys_mem;
  exec init_up { sys_mem.addr = 0xA0000000; sys_mem.size = 0x1000; }
  action a {
    transparent_addr_region_s<> r;
    addr_handle_t h; addr_handle_t h2;
    int va; int va2;
    exec pre_solve {
      r.addr = 0xA0001000;
      r.size = 1024;
      h  = make_handle_from_claim(r, 0);
      va = addr_value(h);
      // 24.7.1.2.1 / Example 316: adding the region to the space yields a
      // handle to that region's address.
      h2  = sys_mem.add_nonallocatable_region(r);
      va2 = addr_value(h2);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a127.pss" <<'PSS'
// 127 — base address space claim type (24.8.1)
component C {
  exec init_up { mem.addr = 0x1000; mem.size = 0x100; }
  action a {
    addr_claim_base_s b;
    constraint b.size == 8;
    rand int x;
  };
}
PSS

cat >"$TMP/a128.pss" <<'PSS'
// 128 — contiguous address space claim type (24.8.2)
component C {
  exec init_up { mem.addr = 0x1000; mem.size = 0x100; }
  action a {
    contiguous_addr_claim_s<> c;
    constraint c.size == 16;
    constraint c.alignment == 4;
    rand int x;
  };
}
PSS

cat >"$TMP/a129.pss" <<'PSS'
// 129 — transparent contiguous address space claim type (24.8.3)
component C {
  exec init_up { mem.addr = 0x1000; mem.size = 0x100; }
  action a {
    transparent_addr_claim_s<> t;
    constraint t.size == 32;
    addr_handle_t h; int av; int early;
    exec pre_solve {
      early = addr_value(make_handle_from_claim(t, 0));
    }
    exec post_solve {
      h  = make_handle_from_claim(t, 0);
      av = addr_value(h);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a130.pss" <<'PSS'
// 130 — address space group (24.9, add_addr_space 24.9.1)
component C {
  addr_space_group_c<> grp;
  contiguous_addr_space_c<> mem0;
  contiguous_addr_space_c<> mem1;
  exec init_up {
    mem0.addr = 0x1000; mem0.size = 0x100;
    mem1.addr = 0x2000; mem1.size = 0x100;
    grp.add_addr_space(mem0);
    grp.add_addr_space(mem1);
  }
  action a {
    contiguous_addr_claim_s<> c;
    constraint c.size == 64;
    rand int x;
  };
}
PSS

cat >"$TMP/a131.pss" <<'PSS'
// 131 — packed_s base struct (24.10.1), little-endian byte order
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    le_s sl;
    addr_handle_t h; addr_handle_t h1; addr_handle_t h2; addr_handle_t h3;
    int b0; int b1; int b2; int b3;
    exec pre_solve {
      sl.A = 1; sl.B = 2; sl.C = 0x1FF; sl.D = 0x7F; sl.E = 0x3F;
      h  = make_handle_from_claim(mem, 0);
      h1 = make_handle_from_handle(h, 1);
      h2 = make_handle_from_handle(h, 2);
      h3 = make_handle_from_handle(h, 3);
      write_struct(h, sl);
      b0 = read8(h); b1 = read8(h1); b2 = read8(h2); b3 = read8(h3);
    }
    rand int x;
  };
}
package p {
  struct le_s : packed_s<LITTLE_ENDIAN> { bit[6] A; bit[2] B; bit[9] C; bit[7] D; bit[6] E; };
}
PSS

cat >"$TMP/a132.pss" <<'PSS'
// 132 — sizeof_s struct (24.10.2)
package p {
  enum color_e { RED = 0, GREEN = 1, BLUE = 2 };
  typedef bit[24] word24_t;
  struct ctrl_s { bit[1] en; bit[3] mode; bit[4] rsvd; };
  struct nested_s { ctrl_s inner; bit[8] tag; };
}
component C {
  action a {
    int struct_bits; int struct_bytes; int nested_bits; int typedef_bits;
    int enum_bits; int int_bits; int bit_bits;
    exec pre_solve {
      struct_bits  = sizeof_s<ctrl_s>::nbits;
      struct_bytes = sizeof_s<ctrl_s>::nbytes;
      nested_bits  = sizeof_s<nested_s>::nbits;
      typedef_bits = sizeof_s<word24_t>::nbits;
      enum_bits    = sizeof_s<color_e>::nbits;
      int_bits     = sizeof_s<int>::nbits;
      bit_bits     = sizeof_s<bit>::nbits;
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a133.pss" <<'PSS'
// 133 — address space handle (24.10.3)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h0; addr_handle_t h1;
    int a0; int a1;
    exec pre_solve {
      h0 = make_handle_from_claim(mem, 0x10);
      h1 = make_handle_from_claim(mem, 0x20);
      a0 = addr_value(h0);
      a1 = addr_value(h1);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a134.pss" <<'PSS'
// 134 — make_handle_from_claim (24.10.4.1)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h0; addr_handle_t h1;
    int a0; int a1;
    exec pre_solve {
      h0 = make_handle_from_claim(mem, 0);
      h1 = make_handle_from_claim(mem, 0x10);
      a0 = addr_value(h0);
      a1 = addr_value(h1);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a135.pss" <<'PSS'
// 135 — make_handle_from_handle (24.10.4.2)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h0; addr_handle_t h1; addr_handle_t h2;
    int a0; int a1; int a2;
    exec pre_solve {
      h0 = make_handle_from_claim(mem, 0x10);
      h1 = make_handle_from_handle(h0, 4);
      h2 = make_handle_from_handle(h1, 0x100);
      a0 = addr_value(h0);
      a1 = addr_value(h1);
      a2 = addr_value(h2);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a136.pss" <<'PSS'
// 136 — addr_value (24.10.5)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h; int av;
    exec pre_solve { h = make_handle_from_claim(mem, 0x24); av = addr_value(h); }
    rand int x;
  };
}
PSS

cat >"$TMP/a137.pss" <<'PSS'
// 137 — addr_value_solve (24.10.6)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h; int av; int avs;
    exec pre_solve { h = make_handle_from_claim(mem, 0x24); av = addr_value(h); avs = addr_value_solve(h); }
    rand int x;
  };
}
PSS

cat >"$TMP/a138.pss" <<'PSS'
// 138 — addr_value_abs (24.10.7): must be the handle's absolute address
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h0; addr_handle_t h1;
    int abs0; int abs1; int av0; int av1;
    exec pre_solve {
      h0 = make_handle_from_claim(mem, 0x10);
      h1 = make_handle_from_claim(mem, 0x800);
      av0  = addr_value(h0);
      av1  = addr_value(h1);
      abs0 = addr_value_abs(h0);
      abs1 = addr_value_abs(h1);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a139.pss" <<'PSS'
// 139 — get_tag (24.10.8): the tag string of the claimed region
package p { struct core_trait_s : addr_trait_s { rand int core; }; }
component C {
  contiguous_addr_space_c<core_trait_s> low_mem;
  contiguous_addr_space_c<core_trait_s> high_mem;
  exec init_up {
    low_mem.addr = 0x1000;  low_mem.size = 0x100;  low_mem.tag = "low";  low_mem.trait.core = 0;
    high_mem.addr = 0x9000; high_mem.size = 0x100; high_mem.tag = "high"; high_mem.trait.core = 1;
  }
  action a {
    transparent_addr_claim_s<core_trait_s> claim;
    string where;
    constraint claim.size == 64;
    constraint claim.trait.core == 1;
    exec post_solve { where = get_tag(make_handle_from_claim(claim, 0)); }
    rand int x;
  };
}
PSS

cat >"$TMP/a140.pss" <<'PSS'
// 140 — primitive reads over a byte addressable space (24.10.9.1)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h; int r8; int r16; int r32; int r64;
    exec pre_solve {
      h = make_handle_from_claim(mem, 0x10);
      write32(h, 0xDEADBEEF);
      r32 = read32(h);
      r8  = read8(h);
      write64(h, 0x0102030405060708);
      r64 = read64(h);
      write16(h, 0x1234);
      r16 = read16(h);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a141.pss" <<'PSS'
// 141 — primitive writes over a byte addressable space (24.10.9.2)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    addr_handle_t h; addr_handle_t h1; addr_handle_t h2; addr_handle_t h3;
    int b0; int b1; int b2; int b3;
    exec pre_solve {
      h  = make_handle_from_claim(mem, 0x10);
      h1 = make_handle_from_handle(h, 1);
      h2 = make_handle_from_handle(h, 2);
      h3 = make_handle_from_handle(h, 3);
      write32(h, 0xDEADBEEF);
      b0 = read8(h); b1 = read8(h1); b2 = read8(h2); b3 = read8(h3);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a142.pss" <<'PSS'
// 142 — read and write a series of bytes (24.10.9.3)
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    list<bit[8]> bytes;
    addr_handle_t h; addr_handle_t h1; addr_handle_t h2;
    int n; int b0; int b1; int b2;
    exec pre_solve {
      h  = make_handle_from_claim(mem, 0x10);
      h1 = make_handle_from_handle(h, 1);
      h2 = make_handle_from_handle(h, 2);
      write_bytes(h, {0x11, 0x22, 0x33});
      read_bytes(h, bytes, 3);
      n  = bytes.size;
      b0 = read8(h); b1 = read8(h1); b2 = read8(h2);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a143.pss" <<'PSS'
// 143 — read and write packed structs (24.10.9.4)
package p { struct ctl_s : packed_s<> { bit[1] en; bit[3] mode; bit[4] rsvd; }; }
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    ctl_s s; ctl_s t;
    addr_handle_t h; int mode_back; int en_back; int byte0;
    exec pre_solve {
      h = make_handle_from_claim(mem, 0);
      s.en = 1; s.mode = 5; s.rsvd = 10;
      write_struct(h, s);
      read_struct(h, t);
      mode_back = t.mode;
      en_back   = t.en;
      byte0     = read8(h);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a144.pss" <<'PSS'
// 144 — primitive operation implementation functions (24.10.9.5)
package p {
  component my_exec_c : executor_c<> {
    function bit[32] read32(addr_handle_t hndl) { return 0xBEEF; }
  }
}
component C {
  my_exec_c ex;
  exec init_up { mem.addr = 0x400; mem.size = 0x100; }
  action a {
    addr_handle_t h; int via_prim;
    exec pre_solve {
      h = make_handle_from_claim(mem, 0);
      write32(h, 0x1234);
      via_prim = read32(h);
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a145.pss" <<'PSS'
// 145 — PSS register definition (24.11.1) and the register access functions
package p {
  struct r0_s : packed_s<> { bit[8] fld0; bit[8] fld1; };
  pure component r0_c : reg_c<r0_s, READWRITE, 16> {};
}
component C {
  r0_c reg;
  action a {
    int r1; int r2; int rv;
    exec pre_solve {
      reg.write_val(0xABCD);
      r1 = reg.read_val();
      reg.write_val_masked(0x00FF, 0x0012);
      r2 = reg.read_val();
      reg.write_field("fld0", 0x7E);
      rv = reg.read_val();
    }
    rand int x;
  };
}
PSS

cat >"$TMP/a146.pss" <<'PSS'
// 146 — PSS register group definition (24.11.2)
package p {
  struct r0_s : packed_s<> { bit[8] fld0; bit[8] fld1; };
  struct rx_s : packed_s<> { bit[8] a; bit[8] b; };
  pure component r0_c : reg_c<r0_s, READWRITE, 16> {};
  pure component my_grp_c : reg_group_c {
    r0_c reg0;
    r0_c reg1;
    reg_c<rx_s, WRITEONLY, 16> regx;
    function bit[64] get_offset_of_instance(string name) {
      match (name) {
        ["reg0"]: return 0x0;
        ["reg1"]: return 0x20;
        ["regx"]: return 0x40;
        default: return -1;
      }
    }
  };
}
component C {
  my_grp_c regs;
  action a {
    int g0; int g1; int gx;
    exec pre_solve {
      regs.reg0.write_val(0x1111);
      regs.reg1.write_val(0x2222);
      regs.regx.write_val(0x3333);
      g0 = regs.reg0.read_val();
      g1 = regs.reg1.read_val();
      gx = regs.regx.read_val();
    }
    rand int x;
  };
}
PSS

# Part B models: an enum and a packed struct, so the backends have something
# structural to emit.
cat >"$TMP/b_model.pss" <<'PSS'
package p {
  enum color_e { RED = 0, GREEN = 1, BLUE = 2 };
  struct cfg_s : packed_s<> { bit[1] en; bit[3] mode; bit[4] rsvd; };
}
component C {
  exec init_up { mem.addr = 0x400; mem.size = 0x1000; }
  action a {
    rand int x;
    rand color_e col;
    constraint col in [RED..BLUE];
    rand cfg_s cfg;
    constraint cfg.mode == 5;
  };
}
PSS

# An invalid model: the component's closing brace is missing (error is on line 5).
cat >"$TMP/b_bad.pss" <<'PSS'
component C {
  action a {
    int x;
  };
PSS

# ============================================================ PART A — 126–146

# A126 — a transparent region: the handle resolves to the region's address, and
# adding the region to a transparent space (24.7.1.2.1 / Ex. 316) yields a
# handle to that same address.
solve_model "$TMP/a126.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A126 "transparent-region" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want va $((0xA0001000))
    want va2 $((0xA0001000))
    verdict A126 "transparent-region"
fi

# A127 — a base claim is first-fit from the region AND is the size it is
# constrained to be.
solve_model "$TMP/a127.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A127 "claim-base" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want b.addr $((0x1000))
    want b.size 8
    verdict A127 "claim-base"
fi

# A128 — a contiguous claim honours size and alignment.
solve_model "$TMP/a128.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A128 "claim-contiguous" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want c.addr $((0x1000))
    want c.size 16
    verdict A128 "claim-contiguous"
fi

# A129 — a transparent claim has no address before the solve and the region's
# absolute address after it.
solve_model "$TMP/a129.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A129 "claim-transparent" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want early 0
    want av $((0x1000))
    want t.addr $((0x1000))
    verdict A129 "claim-transparent"
fi

# A130 — an address space group is accepted and does not disturb allocation:
# the claim still lands in a region it was declared alongside.
solve_model "$TMP/a130.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A130 "addr-space-group" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want c.addr $((0x1000))
    verdict A130 "addr-space-group"
fi

# A131 — packed_s<> lays fields out from the least-significant bit, little-endian:
# A=1(6b) B=2(2b) C=0x1FF(9b) D=0x7F(7b) E=0x3F(6b) => 81 FF FF 3F.
solve_model "$TMP/a131.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A131 "packed-struct" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want b0 $((0x81))
    want b1 $((0xFF))
    want b2 $((0xFF))
    want b3 $((0x3F))
    verdict A131 "packed-struct"
fi

# A132 — sizeof_s: 8 bits / 1 byte for the bit-field struct, and a correct
# non-zero size for a nested struct, a typedef'd bit vector, an enum, int, bit.
solve_model "$TMP/a132.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A132 "sizeof-s" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want struct_bits 8
    want struct_bytes 1
    want nested_bits 16
    want typedef_bits 24
    want int_bits 32
    want bit_bits 1
    wantnz enum_bits
    verdict A132 "sizeof-s"
fi

# A133 — two handles from one region differ by exactly their offset difference.
solve_model "$TMP/a133.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A133 "addr-handle" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want a0 $((0x410))
    want a1 $((0x420))
    verdict A133 "addr-handle"
fi

# A134 — make_handle_from_claim: claim address + offset.
solve_model "$TMP/a134.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A134 "make-handle-from-claim" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want a0 $((0x400))
    want a1 $((0x410))
    verdict A134 "make-handle-from-claim"
fi

# A135 — make_handle_from_handle: argument address + offset, chained.
solve_model "$TMP/a135.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A135 "make-handle-from-handle" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want a0 $((0x410))
    want a1 $((0x414))
    want a2 $((0x514))
    verdict A135 "make-handle-from-handle"
fi

# A136 — addr_value: the handle's address.
solve_model "$TMP/a136.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A136 "addr-value" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want av $((0x424))
    verdict A136 "addr-value"
fi

# A137 — addr_value_solve: the address the handle resolves to for the solve.
solve_model "$TMP/a137.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A137 "addr-value-solve" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want av $((0x424))
    want avs $((0x424))
    verdict A137 "addr-value-solve"
fi

# A138 — addr_value_abs: the handle's absolute address, so it must track
# addr_value for two handles at different addresses (0x410 and 0xC00).
solve_model "$TMP/a138.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A138 "addr-value-abs" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want av0 $((0x410))
    want av1 $((0xC00))
    want abs0 $((0x410))
    want abs1 $((0xC00))
    verdict A138 "addr-value-abs"
fi

# A139 — get_tag: the tag string of the region the claim landed in ("high"),
# which also shows trait matching chose the 0x9000 region.
solve_model "$TMP/a139.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A139 "get-tag" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    wantstr where "high"
    want claim.addr $((0x9000))
    verdict A139 "get-tag"
fi

# A140 — primitive reads: what was written comes back at the right width.
solve_model "$TMP/a140.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A140 "prim-read" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want r32 $((0xDEADBEEF))
    want r8 $((0xEF))
    want r64 $((0x0102030405060708))
    want r16 $((0x1234))
    verdict A140 "prim-read"
fi

# A141 — primitive writes are little-endian: 0xDEADBEEF => EF BE AD DE.
solve_model "$TMP/a141.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A141 "prim-write" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want b0 $((0xEF))
    want b1 $((0xBE))
    want b2 $((0xAD))
    want b3 $((0xDE))
    verdict A141 "prim-write"
fi

# A142 — write_bytes/read_bytes: three bytes written, three read back in order.
solve_model "$TMP/a142.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A142 "byte-series" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want n 3
    want b0 $((0x11))
    want b1 $((0x22))
    want b2 $((0x33))
    verdict A142 "byte-series"
fi

# A143 — write_struct/read_struct round-trips the fields and packs the bytes:
# en=1(1b) mode=5(3b) rsvd=10(4b) => 0xAB.
solve_model "$TMP/a143.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A143 "struct-memory" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want mode_back 5
    want en_back 1
    want byte0 $((0xAB))
    verdict A143 "struct-memory"
fi

# A144 — a user-supplied read32 implementation is used for the primitive call.
solve_model "$TMP/a144.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A144 "prim-implementation" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want via_prim $((0xBEEF))
    verdict A144 "prim-implementation"
fi

# A145 — a register stores what is written, and write_val_masked/write_field
# modify only the masked field: 0xABCD -> 0xAB12 -> 0xAB7E.
solve_model "$TMP/a145.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A145 "register-definition" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want r1 $((0xABCD))
    want r2 $((0xAB12))
    want rv $((0xAB7E))
    verdict A145 "register-definition"
fi

# A146 — a register group's registers are distinct storage and read back.
solve_model "$TMP/a146.pss"
if [ "$RC" -ne 0 ]; then
    fail_item A146 "register-group" "solve failed: $(firstline "$OUT")"
else
    printf '%s' "$OUT" >"$TMP/out.txt"
    want g0 $((0x1111))
    want g1 $((0x2222))
    want gx $((0x3333))
    verdict A146 "register-group"
fi

# ============================================================ PART B — CLI

# B1 — help exits 0 and describes the commands.
OUT=$("$BIN" help 2>&1); RC=$?
if [ "$RC" -ne 0 ]; then
    fail_item B1 "cli-help" "exit $RC"
elif ! printf '%s' "$OUT" | grep -q "USAGE" || ! printf '%s' "$OUT" | grep -q "parse <model>"; then
    fail_item B1 "cli-help" "no USAGE/command list in the help text"
else
    pass_item B1 "cli-help"
fi

# B2 — demo runs the built-in model end to end and prints a summary + trace.
OUT=$("$BIN" demo 2>&1); RC=$?
if [ "$RC" -ne 0 ]; then
    fail_item B2 "cli-demo" "exit $RC"
elif ! printf '%s' "$OUT" | grep -q "Model summary" || ! printf '%s' "$OUT" | grep -q "Satya scenario trace"; then
    fail_item B2 "cli-demo" "no model summary / scenario trace in the demo output"
else
    pass_item B2 "cli-demo"
fi

# B3 — backends exits 0 and lists all six realizations.
OUT=$("$BIN" backends 2>&1); RC=$?
if [ "$RC" -ne 0 ]; then
    fail_item B3 "cli-backends" "exit $RC"
else
    missing=""
    for be in uvm c fw bench json trace; do
        printf '%s\n' "$OUT" | grep -Eq "^$be[[:space:]]" || missing="$missing $be"
    done
    if [ -n "$missing" ]; then
        fail_item B3 "cli-backends" "backends not listed:$missing"
    else
        pass_item B3 "cli-backends"
    fi
fi

# B4 — parse on an invalid model exits non-zero with a line number.
OUT=$("$BIN" parse "$TMP/b_bad.pss" 2>&1); RC=$?
if [ "$RC" -eq 0 ]; then
    fail_item B4 "parse-invalid-line-number" "exit 0 on a model with a missing brace"
elif ! printf '%s' "$OUT" | grep -Eq '\(line [0-9]+'; then
    fail_item B4 "parse-invalid-line-number" "diagnostic carries no line number: $(firstline "$OUT")"
else
    pass_item B4 "parse-invalid-line-number"
fi

# B5 — solve --seed 7 is reproducible byte for byte.
"$BIN" solve "$TMP/b_model.pss" --action a --seed 7 >"$TMP/s7a.txt" 2>&1
rc_a=$?
"$BIN" solve "$TMP/b_model.pss" --action a --seed 7 >"$TMP/s7b.txt" 2>&1
rc_b=$?
if [ "$rc_a" -ne 0 ] || [ "$rc_b" -ne 0 ]; then
    fail_item B5 "seed-7-reproducible" "solve failed (exit $rc_a/$rc_b)"
elif ! cmp -s "$TMP/s7a.txt" "$TMP/s7b.txt"; then
    fail_item B5 "seed-7-reproducible" "two --seed 7 runs differ"
else
    pass_item B5 "seed-7-reproducible"
fi

# B6 — solve --seed 8 differs from --seed 7.
"$BIN" solve "$TMP/b_model.pss" --action a --seed 8 >"$TMP/s8.txt" 2>&1
rc8=$?
if [ "$rc8" -ne 0 ]; then
    fail_item B6 "seed-8-differs" "solve --seed 8 failed (exit $rc8)"
elif cmp -s "$TMP/s7a.txt" "$TMP/s8.txt"; then
    fail_item B6 "seed-8-differs" "--seed 8 output is identical to --seed 7"
else
    pass_item B6 "seed-8-differs"
fi

# braces <file> — prints "ok", or the open/close counts when they differ.
braces() {
    awk '{ o += gsub(/{/, "{"); c += gsub(/}/, "}") }
         END { if (o > 0 && o == c) print "ok"; else printf "open=%d close=%d", o, c }' "$1"
}

gen_backend() { # $1 backend -> $TMP/gen.<backend>, RC
    OUT=$("$BIN" gen "$TMP/b_model.pss" --action a --backend "$1" 2>&1)
    RC=$?
    printf '%s\n' "$OUT" >"$TMP/gen.$1"
}

# B7 — uvm: balanced braces, an enum typedef for the model's enum, a sequence class.
gen_backend uvm
if [ "$RC" -ne 0 ]; then
    fail_item B7 "backend-uvm" "exit $RC: $(firstline "$OUT")"
else
    br=$(braces "$TMP/gen.uvm")
    if [ "$br" != "ok" ]; then
        fail_item B7 "backend-uvm" "unbalanced braces ($br)"
    elif ! grep -q "typedef enum" "$TMP/gen.uvm" || ! grep -q "color_e" "$TMP/gen.uvm"; then
        fail_item B7 "backend-uvm" "no enum typedef for the model's color_e"
    elif ! grep -q "extends uvm_sequence" "$TMP/gen.uvm"; then
        fail_item B7 "backend-uvm" "no uvm_sequence class"
    else
        pass_item B7 "backend-uvm"
    fi
fi

# B8 — c: balanced braces, the packed struct as a C struct, a main().
gen_backend c
if [ "$RC" -ne 0 ]; then
    fail_item B8 "backend-c" "exit $RC: $(firstline "$OUT")"
else
    br=$(braces "$TMP/gen.c")
    if [ "$br" != "ok" ]; then
        fail_item B8 "backend-c" "unbalanced braces ($br)"
    elif ! grep -q "cfg_s_t" "$TMP/gen.c"; then
        fail_item B8 "backend-c" "the model's cfg_s struct is not emitted"
    elif ! grep -q "int main(void)" "$TMP/gen.c"; then
        fail_item B8 "backend-c" "no main()"
    else
        pass_item B8 "backend-c"
    fi
fi

# B9 — fw: an address map carrying the region base, and the interface map.
gen_backend fw
if [ "$RC" -ne 0 ]; then
    fail_item B9 "backend-fw" "exit $RC: $(firstline "$OUT")"
else
    if ! grep -q "## Address map" "$TMP/gen.fw"; then
        fail_item B9 "backend-fw" "no address map section"
    elif ! grep -q "0x400" "$TMP/gen.fw"; then
        fail_item B9 "backend-fw" "the region base 0x400 is missing from the address map"
    elif ! grep -q "## Interface map" "$TMP/gen.fw"; then
        fail_item B9 "backend-fw" "no interface map section"
    else
        pass_item B9 "backend-fw"
    fi
fi

# B10 — bench: the emitted sequence is valid Python and carries the steps.
gen_backend bench
if [ "$RC" -ne 0 ]; then
    fail_item B10 "backend-bench" "exit $RC: $(firstline "$OUT")"
elif ! python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$TMP/gen.bench" 2>"$TMP/pyerr"; then
    fail_item B10 "backend-bench" "emitted sequence is not valid Python: $(firstline "$(cat "$TMP/pyerr")")"
elif ! grep -q "SEQUENCE" "$TMP/gen.bench"; then
    fail_item B10 "backend-bench" "no SEQUENCE table"
else
    pass_item B10 "backend-bench"
fi

# B11 — json: validates with python3 json.load and describes the solved step.
gen_backend json
if [ "$RC" -ne 0 ]; then
    fail_item B11 "backend-json" "exit $RC: $(firstline "$OUT")"
elif ! jverdict=$(python3 - "$TMP/gen.json" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as e:
    print("not valid JSON: %s" % e); raise SystemExit
if d.get("root_action") != "a":
    print("root_action=%r" % d.get("root_action")); raise SystemExit
steps = d.get("steps") or []
if len(steps) != 1:
    print("steps=%d" % len(steps)); raise SystemExit
if steps[0].get("fields", {}).get("cfg.mode") != 5:
    print("cfg.mode=%r" % steps[0].get("fields", {}).get("cfg.mode")); raise SystemExit
print("ok")
PY
); then
    fail_item B11 "backend-json" "python3 rejected the file"
elif [ "$jverdict" != "ok" ]; then
    fail_item B11 "backend-json" "$jverdict"
else
    pass_item B11 "backend-json"
fi

# B12 — trace: the human-readable trace reports the solved step.
gen_backend trace
if [ "$RC" -ne 0 ]; then
    fail_item B12 "backend-trace" "exit $RC: $(firstline "$OUT")"
elif ! grep -q "Satya scenario trace" "$TMP/gen.trace"; then
    fail_item B12 "backend-trace" "no trace header"
elif ! grep -q "solved:" "$TMP/gen.trace"; then
    fail_item B12 "backend-trace" "no solved field values"
elif ! grep -q "constraints:" "$TMP/gen.trace"; then
    fail_item B12 "backend-trace" "no constraint count"
else
    pass_item B12 "backend-trace"
fi

# B13 — the generated scenario JSON is a description, not a model: fed back to
# `parse` it must be rejected.
OUT=$("$BIN" parse "$TMP/gen.json" 2>&1); RC=$?
if [ "$RC" -eq 0 ]; then
    fail_item B13 "scenario-json-rejected" "exit 0 parsing a generated scenario JSON as a model"
elif [ -z "$(printf '%s' "$OUT" | tr -d '[:space:]')" ]; then
    fail_item B13 "scenario-json-rejected" "rejected with no diagnostic"
else
    pass_item B13 "scenario-json-rejected"
fi

# B14 — an unknown backend is an error.
OUT=$("$BIN" gen "$TMP/b_model.pss" --action a --backend nosuchbackend 2>&1); RC=$?
if [ "$RC" -eq 0 ]; then
    fail_item B14 "unknown-backend" "exit 0 for --backend nosuchbackend"
else
    pass_item B14 "unknown-backend"
fi

# B15 — an unknown action is an error.
OUT=$("$BIN" solve "$TMP/b_model.pss" --action nosuchaction 2>&1); RC=$?
if [ "$RC" -eq 0 ]; then
    fail_item B15 "unknown-action" "exit 0 for --action nosuchaction"
else
    pass_item B15 "unknown-action"
fi

# B16 — an unknown command is an error.
OUT=$("$BIN" nosuchcommand 2>&1); RC=$?
if [ "$RC" -eq 0 ]; then
    fail_item B16 "unknown-command" "exit 0 for `nosuchcommand`"
else
    pass_item B16 "unknown-command"
fi

# ------------------------------------------------------------------- summary
printf '\n%d passed, %d failed\n' "$PASSED" "$FAILED"
[ "$FAILED" -eq 0 ] || exit 1
exit 0
