# Checking Satya against OpenTitan

[lowRISC/opentitan](https://github.com/lowRISC/opentitan) is a large open-source
silicon root-of-trust project: SystemVerilog RTL, UVM DV, C firmware, Rust host
tools. It was used as a real-world target to exercise Satya.

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

After the fixes the model solves cleanly — all 13 steps report `9/9 satisfied`,
the generated C compiles and replays the scenario, and the generated bench
driver runs.
