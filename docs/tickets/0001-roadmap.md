# 0001 — Roadmap: from acceptance POC to the compiled backend

The POC proves the approach: generated Rust parsers are tree/behavior-
identical to the walker on the acceptance subset (see
`tests/differential.rs` — generate, `rustc`, run, compare). The compiled
backend exists to close the measured ~60x gap to tree-sitter/pest on the
interpreted path. Milestones in order:

## M1 — Values and captures

- [x] **SHIPPED (2026-10-23)**: the shaped value domain — text, maps
      (`Named`), tagged arrays (`seq`/`rep`/`maybe`) — as a self-contained
      runtime (`Mode::Shape`).
- [x] **SHIPPED (2026-10-24)**: flatten laws V0-V4 (FLAT append, named
      context on the capture's direct value only). The corpus gate
      (PR #2) replays all 32 conformance artifacts — 91 cases, zero
      divergences, zero skips.

## M2 — Terminals beyond literals

- [x] **SHIPPED (2026-10-23)**: `Re` emission — the character-class subset
      via the walker's own `CharacterPattern` (byte tests), the general
      case via the `regex` crate (the generated module declares the
      dependency only when a terminal needs it;
      `requires_regex(grammar)` tells the build).
- [x] **SHIPPED (2026-10-24)**: behind-guards (`!<`/`&<`) — fixed byte
      windows as byte compares, regex forms as end-anchored flanking
      matches through the crate; lookahead and predicates (`!`/`&`) as
      zero-width closures.

## M3 — Injection and rewrite interplay

- [x] **SHIPPED (2026-10-24)**: trivia injection, captured trivia, and
      C9-folded chains — the artifact carries the rewrite as plain wire
      atoms and the generated code re-derives nothing; the corpus gate
      replays every skip/atomic/left-recursion artifact. (The differential
      written for this caught a real walker bug instead:
      parsanol-rs#296 — merged strings across trivia gaps decoded as the
      raw input slice.)
- [ ] Land the explicit trivia/atomic/C9 differential in
      `tests/differential.rs` once parsanol 0.18.20 ships (the test
      compares against the walker, and crates.io 0.18.19 still carries
      the decode bug).

## M4 — Delivery

- [x] **SHIPPED (2026-10-24, PR #3)**: the `parg-codegen` CLI — artifact
      in, Rust module out (`--entry`, `--mode`, `--out`), checksum
      verified; end-to-end tested through compilation and replay.
- [ ] Criterion benches: generated vs walker vs VM on the seven-tool
      corpus; publish the closing-gap number next to the honest table.
- [ ] Decide the shipping shape (owner decision): a proc-macro
      (`parg_parser!(artifact)`), a build-time crate, or both.

## Acceptance (bar for "the backend exists")

The whole conformance corpus replays byte-identically through generated
parsers in CI, and the benchmark table shows the generated backend's
number beside the interpreted one.
