# Phase 4 — Executor ✅

- Created `src/moss/executor.rs`.
- `Executor` is a **zero-size unit struct** (`#[derive(Clone, Copy)]`) — stateless, no sandbox dir.
- `run(&self, gap: &Gap, artifact: &Artifact, blackboard: &Blackboard) -> Result<()>`:
  - Script path: writes code to `tempfile::NamedTempFile` (auto-deleted on drop), spawns interpreter via `tokio::process::Command`, wraps in `tokio::time::timeout`.
  - Language → interpreter mapping: `python/python3 → python3`, `shell/sh/bash → sh`, `javascript/js → node`, any other string → passed through directly.
  - stdout parsed as JSON → `EvidenceStatus::Success`. Non-JSON stdout → `EvidenceStatus::Partial`. Non-zero exit → `EvidenceStatus::Failure`. Timeout → `EvidenceStatus::Failure`.
  - Agent path: stub — writes `Failure` evidence, returns `Ok(())`.
- Evidence written directly to Blackboard (Blackboard pattern).
- Unit tests using real `sh` subprocesses.

**Files:** `src/moss/executor.rs` (new), `src/moss/mod.rs`, `Cargo.toml` (+tempfile)
