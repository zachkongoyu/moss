# Phase 3 — Compiler ✅

- Created `src/moss/compiler.rs`.
- `Artifact` enum with `#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]`:
  - `Script { language: Box<str>, code: Box<str>, timeout_secs: u64 }`
  - `Agent { role, goal, tools: Vec<Box<str>>, instructions }`
- `Compiler { provider: Arc<dyn Provider> }`.
- `compile(&self, gap: &Gap, prior_attempts: &[Box<str>]) -> Result<Artifact>`: renders `prompts/compiler.md`, calls LLM, deserializes.
- Prompt (`compiler.md`): language-agnostic — LLM picks best language for each gap. `prior_attempts` injected so LLM avoids repeating past mistakes.
- Unit tests via `MockCompilerProvider` (no real HTTP). Tests: Script, Agent, markdown fence stripping, prior attempts.

**Files:** `src/moss/compiler.rs` (new), `src/moss/prompts/compiler.md` (new), `src/moss/mod.rs`
