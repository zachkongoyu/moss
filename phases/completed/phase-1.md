# Phase 1 — Rebuild Blackboard ✅

- Full rewrite of `blackboard.rs`.
- `GapState`: `Blocked | Ready | Assigned | Gated | Closed`
- `GapType`: `Proactive | Reactive`
- `Gap`: `gap_id`, `name`, `state`, `description`, `gap_type`, `dependencies: Vec<Box<str>>`, `constraints: Option<Value>`, `expected_output: Option<Box<str>>`
- `EvidenceStatus`: `Success | Failure { reason } | Partial`
- `Evidence`: `gap_id`, `attempt: u32`, `content: Value`, `status`
- `Blackboard`: `intent: Mutex<Option<Box<str>>>`, `gaps: DashMap<Uuid, Gap>`, `name_index`, `evidences`, `gates`
- Methods: `insert_gap`, `set_gap_state`, `append_evidence`, `get_gap`, `get_gap_id_by_name`, `get_evidence`, `drain_ready`, `promote_unblocked`, `all_closed`, `insert_gate`, `set_intent`, `all_evidence`, `status_summary`. (`all_gated_or_closed` removed in Phase 6 — see ADR-007.)
- All fields `pub(crate)` via getters. Derives `Debug`.
- Unit tests: linear chain, parallel fanout, gated/closed, evidence.

**Files:** `src/moss/blackboard.rs`
