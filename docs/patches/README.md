# Held patches

Work produced in parallel sessions that is **not merged into `main`**, kept here
so it is not lost and so the next session starts from evidence rather than from
scratch.

Each patch was verified by its author against commit `c92d8d2` (v0.75.0) and its
report records the before/after per proof row. What is merged into `main` today
is a subset; the rest are staged here.

## Merged into v0.76.0 — applied, verified, shipped

| patch | rows | notes |
|---|---|---|
| `fix-engine-constructs.patch` | 79, 83 | merged |
| `fix-symbols.patch` | 84, 102, 104 | merged |
| `fix-bdd-relations.patch` | — | var-to-var comparisons in the BDD prover; `--order` now differs |
| `docs-reconcile.patch` | — | 21 stale `FEATURES` rows corrected; `CORPUS.md` corrected to 0-of-113 |
| `fix-collections.patch` | 6, 15 | merged |
| `fix-forall-template.patch` | 58, 25 | merged |
| `fix-unsigned.patch` | 9 | merged |

## Held — verified by their author, NOT merged

### `fix-ast-gaps.patch` — rows 43, 80, 100, 103

All four were verified passing against `c92d8d2` by the agent that wrote it. It
does not apply on top of the merged tree: it changes `dispatch` to take a `ctx`
parameter and adds `symbol_scopes` / `override_scopes` to the engine, which
overlaps the dispatch changes in `fix-engine-constructs` and `fix-symbols`.

The conflict is structural, not textual — resolving it means re-deriving the
`dispatch` signature in the current `src/engine.rs`, not re-running `git apply`.
`src/engine.rs.rej` from the failed attempt is not kept; the hunk contents are in
the patch itself.

Note recorded by that agent, and worth keeping: **row 43's fixture was correct**.
PSS 3.1 Annex B.2 lists `symbol_declaration` under `action_body_item`, so a
`symbol` inside an action body is legal PSS — the grammar was widened, the
fixture was not bent. Row 103's real defect was a broken assertion
(`m103_body` required an unconstrained `rand int kept` to equal 0, which no seed
satisfies), not the exec-body gap originally diagnosed.

### `sweep-speed.patch` — no rows, 3.1x on a full proof run

`run_all.sh` 158.4 s → 50.3 s with byte-identical output and the counts
unchanged. Conflicts on `tests/feature_proof/proof-slice-3.sh` because the proof
fixtures were edited by other patches in the same region.

Worth reading for the design note: its first attempt was **slower** (30 s → 41 s)
because it spawned a process per seed to split the swept output, and in this
environment any process costs ~21 ms. The version that works splits the sweep
once into a table and walks it with shell builtins.

## How to apply one

```bash
git checkout -b merge/<name> main
git apply --3way docs/patches/<name>.patch
# resolve, then:
cargo test && ./tests/feature_proof/run_all.sh ./target/release/satya
```

Expect `138 pass / 24 fail` from `main` today; each patch's report states what it
should add.
