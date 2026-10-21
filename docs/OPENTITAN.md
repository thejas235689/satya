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

So Satya cannot be run *directly* against OpenTitan. What it can be run against
is a model **written to OpenTitan's published register interface** — which is
what `examples/opentitan/ot_uart.pss` is.

## 2. The UART model

`examples/opentitan/ot_uart.pss` models OpenTitan's `uart` IP. The register map,
field names and bit positions are taken from
`hw/ip/uart/data/uart.hjson` (Apache-2.0); the byte offsets are the ones reggen
assigns in declaration order (4-byte stride).

| Register | Offset | Modelled as |
|---|---|---|
| `CTRL` | 0x00 | `uart_ctrl_s` — `TX`, `RX`, `NF`, `SLPBK`, `LLPBK`, `PARITY_EN`, `PARITY_ODD`, `NCO[31:16]` |
| `RDATA` | 0x04 | `read_byte` action (`swaccess ro`) |
| `WDATA` | 0x08 | `write_byte` action (`swaccess wo`) |
| `FIFO_CTRL` | 0x0C | `uart_fifo_ctrl_s` — `RXILVL[4:2]`, `TXILVL[7:5]` |
| `FIFO_STATUS` | 0x10 | `poll_status` action |
| `OVRD` | 0x14 | `set_override` action |
| `VAL` | 0x18 | — |
| `TIMEOUT_CTRL` | 0x1C | — |

Two scenarios are modelled: `uart_smoke_test` (configure, a parallel burst of
`write_byte` under a `repeat`, `poll_status`, `read_byte`, `set_override`) and
`uart_soak_test` (interleaved configure/write/read). Flow objects are recycled
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

## 3. What it found

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
  `unsatisfied` list), instead of only a count.
* The `fw` backend uses the same dependency ordering for its header.

After the fixes the model solves cleanly — every step reports `n/n satisfied`,
the generated C compiles and replays the scenario, and the generated bench
driver runs.
