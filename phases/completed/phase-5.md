# Phase 5 — Runner + Observability ✅

- Created `src/moss/runner.rs`.
- `Runner { compiler: Arc<Compiler> }` — no `Arc<Executor>` (Executor is zero-cost to construct).
- `run(&self, blackboard: Arc<Blackboard>) -> Result<(), MossError>`:
  - Loop: `promote_unblocked → drain_ready → JoinSet fan-out → join_next`.
  - Each task: check `attempt_count >= MAX_RETRIES (3)` → force close. Else compile → execute → close or set back to Ready for retry.
  - Termination: `drain_ready` empty + `all_closed` → `Ok(())`. Empty + not all closed → `Err(Deadlock)`.
  - Note: Rewritten in Phase 6 to persistent JoinSet with one-completion-per-iteration loop. `all_gated_or_closed` removed.
- `MAX_RETRIES = 3`. Failed gaps set back to `Ready`; Runner picks them up next round.
- Wired into `Moss::run()` in `lib.rs`: decompose → `runner.run(Arc::clone(&blackboard))` → synthesize.
- Added `tracing` + `tracing-subscriber` (pretty format). Log levels:
  - `RUST_LOG=moss=info` — pipeline flow (intent, gap open/close)
  - `RUST_LOG=moss=debug` — + evidence + full blackboard state each round
  - `RUST_LOG=moss=trace` — + gap detail before compile + blackboard after each execution

**Files:** `src/moss/runner.rs` (new), `src/moss/mod.rs`, `src/lib.rs`, `src/main.rs`, `Cargo.toml` (+tracing)
