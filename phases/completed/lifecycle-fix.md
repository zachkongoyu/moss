# Blackboard Lifecycle Fix ✅

*Small fix, high priority — landed before Phase 6.*

- Move `Arc<Blackboard>` creation into `Moss::run()` (fresh per query).
- Remove `blackboard` field from `Moss` struct.
- Each query gets an isolated Blackboard; prior query state does not leak.

**Files:** `src/lib.rs`
