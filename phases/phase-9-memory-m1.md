# Phase 9 — Memory M1

**Status:** Planned
**Effort:** Small-medium.

---

## Spec

Multi-turn conversation within a session.

- `SessionBuffer { entries: VecDeque<SessionEntry>, capacity: usize }`
- `m1_recent()` returns last N entries as `Vec<Message>`.
- Wire into `Moss::run()` — update buffer after each query, inject into `orchestrator.decompose`.

**Files:** `src/memory/` (new), `src/main.rs`
