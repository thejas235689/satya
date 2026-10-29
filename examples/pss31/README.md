# PSS 3.1 target-runtime examples

Two models from the PSS 3.1 public-review write-up on the **target runtime**:
`sync_pkg` channels (21.9) and `export function` (20.4.2).

Both are reproduced as published, so they can be used to check what Satya
actually does with them.

## What Satya does with them

Run `satya parse` and `satya solve <file> --action <name>`.

| example | parses | schedules | channels | export function |
|---|---|---|---|---|
| `noc_channel.pss` | yes | yes — 2 steps in `parallel`, `strm`/`threshold` solved | **no** | n/a |
| `dma_export_irq.pss` | yes | yes | **no** | **no** |

Both parse, and the NoC model even produces a correct scenario:

```
0. load_a     @ noc_c  [parallel g1]   solved: strm=69 threshold=79
1. inject_a   @ noc_c  [parallel g1]
```

But the two things the models exist to demonstrate are **not implemented**:

- **`channel_c`** appears nowhere in the engine, IR or solver. `put`, `get`,
  `try_put` and `try_get` are parsed as ordinary calls and then ignored, so the
  handshake in `noc_channel.pss` does nothing and `inject_a` never receives a
  stream id.
- **`export function`** has no implementation, because it needs something for
  the environment to call *into* — a target runtime. Satya has none.

`import target function` is parsed and the declarations are recorded; the calls
are emitted by the backends rather than executed.

## Why this is recorded rather than fixed

Implementing `channel_c` properly means a solve-time FIFO *and* a decision about
blocking. Satya runs `exec` bodies to completion in one pass, so there is no
place for a `get()` to block and no other exec block to keep running while it
does — which is exactly the obligation 20.8 places on the tool. A FIFO alone
would make the example look like it works while the semantics it depends on are
absent.

An attempt at the FIFO alone was made and removed: it did not fire (the channel
field's type was not matched), and a version that compiles and does not fire is
worse than none.

This is the honest state of the largest remaining gap in the tool, and it is the
one that matters most for PSS 3.1: everything else Satya does is settled before
the test starts.
