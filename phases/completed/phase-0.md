# Phase 0 — Clean the Foundation ✅

- Removed unused deps (`async-openai`, `sqlx`). Added `thiserror`, `minijinja`.
- Created `src/error.rs` with `MossError` and `ProviderError` via `thiserror`.
- Updated `Provider` trait: `complete_chat` returns `Result<String, ProviderError>`.
- Replaced all `.expect()` / `panic!()` with `?` in `openrouter.rs`.
- `main.rs` handles errors to stderr, keeps loop alive.

**Files:** `Cargo.toml`, `src/error.rs`, `src/providers/mod.rs`, `src/providers/remote/openrouter.rs`, `src/providers/local/mod.rs`, `src/main.rs`
