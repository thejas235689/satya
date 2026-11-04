# Checking Satya against OpenTitan

[lowRISC/opentitan](https://github.com/lowRISC/opentitan) is a large open-source
silicon root-of-trust project: SystemVerilog RTL, UVM DV, C firmware, Rust host
tools. It was used as a real-world target to exercise Satya — first a simple IP
(**UART**, section 2), then a deliberately hard one (**AES**, section 5).

## 1. OpenTitan contains no PSS

Satya consumes PSS (Portable Test and Stimulus) models. OpenTitan does not ship
any:

| Query (GitHub code search, default branch) | Results |
|---|---|
| `repo:lowRISC/opentitan extension:pss` | **0** |
| `repo:lowRISC/opentitan "portable stimulus"` | **0** |

A bare `pss` search returns 32 hits, but they are all false positives — matches
inside RSA crypto sources, register headers and generated files. There is no PSS
model, and no PSS tooling integration, in the repository.

The register headers Satya could otherwise be compared against (`hw/ip/uart/sw/
autogen/uart_regs.h`, `sw/device/lib/dif/autogen/dif_uart_autogen.h`) are *not*
checked in — OpenTitan generates them at build time from HJSON. The HJSON **is**
the authoritative source, and that is what the model is checked against.

So Satya cannot be run *directly* against OpenTitan. What it can be run against
is a model **written to OpenTitan's published register interface** — which is
what `examples/opentitan/ot_uart.pss` is.

## 2. The UART model

`examples/opentitan/ot_uart.pss` models OpenTitan's `uart` IP. The register map,
field names, bit ranges and software-access attributes are taken from
`hw/ip/uart/data/uart.hjson` (Apache-2.0); the byte offsets are the ones reggen
assigns in declaration order (4-byte stride).

| Register | Offset | SW | Fields modelled |
|---|---|---|---|
| `CTRL` | 0x00 | rw | `TX[0]` `RX[1]` `NF[2]` `SLPBK[4]` `LLPBK[5]` `PARITY_EN[6]` `PARITY_ODD[7]` `RXBLVL[9:8]` `NCO[31:16]` |
| `RDATA` | 0x04 | ro | whole register (`read_byte`) |
| `WDATA` | 0x08 | wo | whole register (`write_byte`) |
| `FIFO_CTRL` | 0x0C | rw | `RXILVL[4:2]` `TXILVL[7:5]` |
| `FIFO_STATUS` | 0x10 | ro | `TXLVL[7:0]` `RXLVL[23:16]` |
| `OVRD` | 0x14 | rw | `TXEN[0]` `TXVAL[1]` |
| `VAL` | 0x18 | ro | `RX[15:0]` |
| `TIMEOUT_CTRL` | 0x1C | rw | `VAL[23:0]` `EN[31]` |

Struct fields are the HJSON names lower-cased, so the model can be diffed
mechanically against the spec.

Two scenarios are modelled: `uart_smoke_test` (configure, a parallel burst of
`write_byte` under a `repeat`, `poll_status`, `read_byte`, `read_value`,
`set_timeout`, `set_override`) and `uart_soak_test`. Flow objects are recycled
through a pool, and the UART address window is declared in an `exec init_up`
block so the firmware and bench backends report real addresses.

Two things about the schedule are worth stating precisely, because they are not
what the source reads like. `poll_status` is written after the burst but is
carried into the *same* concurrent group as the eight `write_byte` steps
(`[parallel g1]` on all nine) — the join-specification defect the audit records,
where the statement after a concurrent block lands inside it. And the recycled
config object is shared but not *carried*: every `write_byte` takes
`config_pool` object #0, yet its fields are solved independently of the object
the `configure` step filled (audit row 48), so `frame.stop_bits` varies per step
instead of following the configuration.

Run it:

```bash
satya parse examples/opentitan/ot_uart.pss
satya solve examples/opentitan/ot_uart.pss --action uart_smoke_test
for b in uvm c fw bench json trace; do
  satya gen examples/opentitan/ot_uart.pss --action uart_smoke_test --backend $b -o ot_uart.$b
done
```

## 3. Model fidelity, checked mechanically

`tools/ot_regmap_check.py` extracts the register map from `uart.hjson` and
compares it against the model, field by field and width by width:

```
$ python3 tools/ot_regmap_check.py
OpenTitan UART: 8 registers, 6 modelled structs

REGISTER       SW     RESULT
------------------------------------------------------------------------
CTRL           rw     match (9 fields)
RDATA          ro     (whole-register, no named fields)
WDATA          wo     (whole-register, no named fields)
FIFO_CTRL      rw     match (2 fields)
FIFO_STATUS    ro     match (2 fields)
OVRD           rw     match (2 fields)
VAL            ro     match (1 fields)
TIMEOUT_CTRL   rw     match (2 fields)
------------------------------------------------------------------------
registers matching: 6   mismatching: 0
```

That check is worth having because the first draft of the model **was wrong in
three ways**, and only the mechanical diff caught them:

1. **`CTRL.RXBLVL[9:8]` was missing** — an entire 2-bit field was absent.
2. **`OVRD` had the wrong fields** — the model invented `tx_enable`/`rx_enable`,
   where the spec has `TXEN[0]` and `TXVAL[1]` (there is no RX bit in OVRD).
3. **`VAL` and `TIMEOUT_CTRL` were not modelled at all** — two of the eight
   registers had no struct.

Reading the HJSON by eye was not enough; the diff was.

## 4. Defects this work exposed in Satya

Modelling a real IP surfaced **three** further defects, all fixed:

1. **Inline ranges on struct fields were dropped.** `rand int in [0..7] stop_bits;`
   inside a `buffer` produced no constraint, so the field solved to `854`.
   Action members were handled; type members were not.
2. **Implications were checked but never propagated.** `parity_odd == 1 ->
   parity_enable == 1` was reported unsatisfied instead of being enforced, so a
   legal-looking configuration came out inconsistent.
3. **The generated C did not compile for nested structs.** Typedefs were emitted
   in alphabetical order, so `outer_b_t` referenced `inner_s_t` before it was
   defined. Types are now emitted in dependency order.

Two further improvements came out of the same work:

* The engine now **reports which constraint** could not be satisfied (the trace
  prints `not satisfied: <constraint>`, and the JSON backend carries an
  `unsatisfied` list), instead of only a count. This is what made defect 2 a
  one-step diagnosis.
* The `fw` backend uses the same dependency ordering for its header.

After the fixes the model solves cleanly — all 13 steps run and report every
constraint they declare satisfied (`9/9` on the configured actions, `8/8` on
`poll_status` and `read_byte`, `1/1` on `set_timeout`, and the two actions that
declare none report none), the generated C compiles and replays the scenario, and
the generated bench driver runs (13 steps, 0 unresolved).

---

## 5. A harder target: AES

`examples/opentitan/ot_aes.pss` models the `aes` IP from the same
`hw/ip/aes/data/aes.hjson` source. AES is a much harder model than UART because
it uses four constructs UART does not:

| Feature | Where | Modelled? |
|---|---|---|
| **Multiregs** (register arrays) | `KEY_SHARE0/1` ×8, `IV` ×4, `DATA_IN` ×4, `DATA_OUT` ×4 | **partly** — see below |
| **Shadowed** registers (write value, then its complement) | `CTRL_SHADOWED`, `CTRL_AUX_SHADOWED`, `CTRL_GCM_SHADOWED` | yes |
| **REGWEN gate** (`rw0c`) | `CTRL_AUX_REGWEN` | yes |
| **Enum-typed fields** | `OPERATION`, `MODE`, `KEY_LEN`, `PRNG_RESEED_RATE` | yes |

The model parses (4 enums, 8 types, 9 actions) and both scenarios schedule — 10
steps, 0 warnings, every declared constraint reported satisfied (`10/10` on
`program_ctrl`). One constraint in the model is *not* evaluated, though:
`load_key`'s `key.words.sum() >= 1` comes back as `not evaluated: <expr>`
because the multireg field it names never becomes a variable — see below. The
shadowed-write protocol is expressed as an activity plus a constraint
(`complement == ~value`), and the REGWEN gate as an action ordering. All six
backends generate, and the generated C compiles and runs.

### Where it strains

**Multiregs — resolved in 0.6.0 for a literal count.** AES declares `KEY_SHARE0` as a multireg with
`count: "NumRegsKey"` — eight registers, not one. When this model was first
written, Satya parsed `array<T, N>` but gave arrays **no semantics**: such a
field collapsed to a single scalar. The model therefore hand-unrolled it into
`word0 … word7`.

Arrays now expand into one variable per element with per-element domains
(PSS 8.4) **when the element count is a literal**, so the model declares the
multireg directly:

```pss
struct aes_key_share0_s {
   rand array<bit[32], NUM_REGS_KEY> words;   // KEY_SHARE0_0 .. KEY_SHARE0_7
};
```

and constrains individual words (`key.words[0] == 0x2b7e1516`) and aggregates
(`key.words.sum() >= 1`). `foreach (arr[i])` iterates elements.

The expansion does not survive a *named* count, and this model uses one:
`NUM_REGS_KEY` is a `static const int`. With a literal the eight words really do
become eight variables — `array<bit[32], 4>` and `bit[32] w[4]` both solve per
element, and `sum()` is enforced on them. With `array<bit[32], NUM_REGS_KEY>` the
field yields **no variables at all** in a struct: `load_key` reports
`constraints: 8/8 satisfied` and `not evaluated: <expr>` while `key.words[0]`
appears nowhere in the solved fields, so the FIPS-197 first word is never
written. That is the audit's worst pattern — success reported while nothing
binds — reached here through a parameterised multireg. Declaring the field as
`rand bit[32] words[NUM_REGS_KEY];` or with the literal `8` works; the
`array<T, N>` form with a symbolic `N` does not.

This was the honest answer to *"where would it break on a large production
model?"* — not the parser, and not the scheduler, but **anywhere the register
file is an array**. It is closed for fixed-size arrays with a literal count;
what remains is the symbolic-count form above and the list of approximations in
`docs/SEMANTICS.md` §2.

### Three defects AES exposed

1. **Part-selects were not parsed.** PSS 8.6.1 defines
   `identifier [ constant_expression : constant_expression ]`. Satya parsed
   `x[i]` but not `x[msb:lsb]`, so `shadow.value[15:0]` was a syntax error.
   Now parsed, and evaluated as a slice of a known value.
2. **Bit-wise complement could not be solved.** `complement == ~value` was left
   unsatisfied — and fixing it needed three separate corrections:
   * the repair pass *clamped* the computed value into the field's domain, so
     `~x` became `0` for every 32-bit `x`; it now **wraps** into an unsigned
     field (two's-complement truncation), which is what `~` means on a `bit[N]`;
   * `bit[N]` domains were capped at 20 bits, which truncated the wrap — they
     now use the declared width (up to 62 bits);
   * the checker evaluated `~` as a 64-bit value while the assignment produced a
     32-bit one, so the two disagreed; bit-wise results are now compared at the
     field's width.
3. **Nothing else.** The scheduler, pools, arbitration and the six backends all
   handled AES unchanged.

After the fixes `program_ctrl` reports `10/10 satisfied`, with
`value = 0x12348d01` and `complement = 0xedcb72fe` — the correct 32-bit
complement.
