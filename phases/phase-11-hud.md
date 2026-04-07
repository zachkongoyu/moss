# Phase 11 — HUD

**Status:** Planned
**Effort:** Small. Just another `SignalBus` consumer.

---

## Spec

Subscribes to `SignalBus` from Phase 6. No new infrastructure needed.

- `main.rs` spawns a HUD task: `let mut rx = bus.subscribe();`
- Renders every `Signal` variant as a terminal delta (colored status, progress bars, gate prompts).
- Just another consumer of the same bus — plug in and go.

**Files:** `src/main.rs`
