# Phase 2 — Orchestrator + Moss Facade ✅

- Created `src/moss/decomposition.rs` (was `plan.rs` — renamed for clarity).
  - `Decomposition { intent: Option<String>, gaps: Option<Vec<GapSpec>> }`
  - `GapSpec { name, description, gap_type, dependencies, constraints, expected_output }` — all `String` fields (short-lived DTO).
- Rewrote `orchestrator.rs`:
  - `decompose`: renders `prompts/decompose.md` via minijinja, calls LLM, strips markdown fences, deserializes into `Decomposition`.
  - `synthesize`: renders `prompts/synthesize.md`, passes real evidence from `blackboard.all_evidence()`.
- Prompt format: Markdown instructions + XML-tagged input variables. More portable across LLM providers than pure XML.
- Created `src/lib.rs` — `Moss` facade (the only `pub` entry point):
  - `Moss::new(provider)` wires Orchestrator + Runner + Blackboard.
  - `Moss::run(query)` → decompose → execute → synthesize → return answer.
- `main.rs` simplified: only uses `Moss`.

**Files:** `src/moss/decomposition.rs` (new), `src/moss/orchestrator.rs`, `src/moss/mod.rs`, `src/lib.rs`, `src/main.rs`
