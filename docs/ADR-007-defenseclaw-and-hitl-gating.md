# ADR-007: DefenseClaw Build & HITL Gating as I/O

**Status:** Proposed
**Date:** 2026-04-07
**Deciders:** Zach (architect), contributors
**Relates to:** ADR-005 (HITL via `GapState::Gated`), ARCHITECTURE.md §8 (DefenseClaw), §3 (Core Runtime Loop)

---

## Context

Two things need to happen:

1. **DefenseClaw** needs to be built from scratch as an L0 component.
2. **The Runner's execution model** needs to handle HITL (human-in-the-loop) gating without special-casing it.

### Gated is just I/O

A gap waiting for human approval is no different from a gap waiting for a webpage to load. Both are I/O. The human is just another async resource the gap might need to await.

This means:

- The gap task **stays alive on the JoinSet** while waiting for the human. It doesn't return early and set a special state.
- The Runner doesn't need to know about gating at all. It sees tasks on the JoinSet. Some finish fast, some take a while. It doesn't care why.
- `all_gated_or_closed()` disappears. The terminal condition is just `all_closed()`.
- `Gated` remains a `GapState` for **observability only** (HUD, planner view, CLI display). The Runner never checks for it.

### Gated is not just security

`GapState::Gated` means "this gap needs a human to perform some action before it can proceed." That includes security approval (DefenseClaw), but also: entering a 2FA code, picking from options, confirming a preference, plugging in a device — anything where the system hits a point that requires human action.

Any component can trigger it: DefenseClaw (pre-execution), the Executor (mid-execution), a MicroAgent (during a ReAct loop).

### The current Runner can't handle this

The current Runner drains the **entire** JoinSet before looping:

```rust
// Current — drains ALL tasks before checking for new Ready gaps
while let Some(result) = tasks.join_next().await { ... }
```

If a gated gap sits on the JoinSet awaiting human input (which could be minutes), this blocks the entire loop. No new Ready gaps get dispatched. The fix: process completions **one at a time**, checking for new work between each.

---

## Decision

### 1. Human approval is awaited inside the gap task — just I/O

When DefenseClaw (or any component) gates a gap, the task doesn't return. It sets the Blackboard state to `Gated`, fires a broadcast notification so the CLI can prompt the user, then awaits on a channel for the response:

```rust
tasks.spawn(async move {
    let artifact = compiler.compile(&gap, &prior).await?;

    match defense_claw.scan(&artifact, gap.constraints()) {
        ScanVerdict::Approved => { /* proceed to execute */ }
        ScanVerdict::Gated { reason } => {
            bb.set_gap_state(&gap.gap_id(), GapState::Gated)?;
            bb.insert_gate(gap.gap_id(), reason);  // → fires broadcast to CLI

            // Wait for human — this is just I/O, like awaiting a web response
            let approved = gate_rx.recv().await;

            if approved {
                bb.set_gap_state(&gap.gap_id(), GapState::Assigned)?;
                // fall through to execute
            } else {
                bb.append_evidence(/* rejection evidence */);
                bb.set_gap_state(&gap.gap_id(), GapState::Closed)?;
                return Ok(());
            }
        }
        ScanVerdict::Rejected { reason } => {
            bb.append_evidence(/* failure evidence */);
            bb.set_gap_state(&gap.gap_id(), GapState::Closed)?;
            return Ok(());
        }
    }

    Executor::new().run(&gap, &artifact, &bb).await?;
    // ... success/failure handling, set Closed ...
});
```

The task stays on the JoinSet. Other tasks keep executing. When the human responds, the task resumes, executes, posts evidence, closes. From the Runner's perspective, nothing special happened — a task just took a while.

### 2. Runner processes completions one at a time

```rust
pub(crate) async fn run(&self, blackboard: Arc<Blackboard>) -> Result<(), MossError> {
    let mut tasks: JoinSet<Result<(), MossError>> = JoinSet::new();

    loop {
        blackboard.promote_unblocked();
        let ready = blackboard.drain_ready();

        for gap in ready {
            let compiler = Arc::clone(&self.compiler);
            let bb = Arc::clone(&blackboard);
            tasks.spawn(async move {
                // compile → scan → maybe await human → execute → evidence → close
            });
        }

        if tasks.is_empty() {
            return if blackboard.all_closed() {
                Ok(())
            } else {
                Err(MossError::Deadlock)
            };
        }

        // Wait for ONE task to complete — not all
        if let Some(result) = tasks.join_next().await {
            result.map_err(|e| MossError::Blackboard(format!("task panicked: {e}")))??;
        }

        // Loop back: promote newly unblocked gaps, drain, spawn, wait for next
    }
}
```

Each iteration: promote → drain → spawn → wait for **one** completion → loop. A gated gap sits on the JoinSet. Other gaps complete, their dependents get promoted and dispatched. When the gated gap's human responds, it completes, loop promotes its dependents.

No `all_gated_or_closed()`. No `has_gated()`. No special gate handling. Just `all_closed()`.

### 3. Blackboard emits signals via SignalBus

The Blackboard holds a `SignalBus` (see ADR-008). `insert_gate()` emits `Signal::GateRequested` on the bus. The CLI subscribes and shows the prompt instantly — while other gaps keep running. The bus is a system-level primitive, not Blackboard-specific. Any component that gates a gap uses `insert_gate()` and the notification fires automatically. The CLI subscribes and surfaces it. The gap task awaits on its own `oneshot` channel for the response.

### 4. Gate response channel: per-gap `oneshot`

Each gated gap needs to receive its specific approval/rejection. A `tokio::oneshot` per gate is the simplest model:

```rust
let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
bb.insert_gate(gap.gap_id(), reason, tx);  // store the sender
// ...
let approved = rx.await.unwrap_or(false);  // gap awaits its specific response
```

The CLI, upon `approve <name>` or `reject <name>`, looks up the gate and sends on its `oneshot::Sender`. Clean, no routing logic needed.

### 5. DefenseClaw — built from scratch as L0

DefenseClaw is one producer of gates. Its internal design is unchanged from ARCHITECTURE.md §8:

```rust
pub(crate) struct DefenseClaw {
    blocklist: Vec<Pattern>,
    max_script_size: usize,
}

pub(crate) enum ScanVerdict {
    Approved,
    Gated { reason: String },
    Rejected { reason: String },
}

impl DefenseClaw {
    pub(crate) fn scan(&self, artifact: &Artifact, constraints: Option<&Value>) -> ScanVerdict;
}
```

Four-stage pipeline:

| Stage | Check | Verdict |
|-------|-------|---------|
| 1. Static analysis | Forbidden imports, network calls in Proactive scripts, writes outside sandbox | Rejected |
| 2. Capability check | Artifact capabilities vs. Gap constraints | Rejected |
| 3. Resource bounds | Timeout and memory limits set | Rejected |
| 4. HITL gate | High-risk action patterns (email, delete, purchase) | Gated |

---

## Changes to Existing Design

### Runner (§3, runner.rs)

| Aspect | Before | After |
|--------|--------|-------|
| JoinSet drain | Drain all, then loop | Wait for one, then loop |
| Terminal condition | `all_gated_or_closed()` → Ok | `all_closed()` → Ok |
| Gate awareness | Knows about Gated state | Doesn't know or care |
| JoinSet lifetime | Created fresh each round | Persistent across rounds |
| `all_gated_or_closed()` | Used as terminal check | **Removed** |

### Blackboard (§4.2, blackboard.rs)

| Aspect | Before | After |
|--------|--------|-------|
| `insert_gate()` | Stores payload in DashMap | Emits `Signal::GateRequested` via `SignalBus` (ADR-008), stores `oneshot::Sender`, returns `oneshot::Receiver` |
| `all_gated_or_closed()` | Exists | **Removed** — no longer needed |
| Signal Bus | Not present | `SignalBus` threaded through system (ADR-008). Blackboard mutations auto-emit. |

### GapState::Gated (ADR-005)

Remains a first-class state. Still set when a gap needs human action. Still visible on the Blackboard for HUD/planner view. The only change: the Runner doesn't check for it. It's purely observability.

### CLI (§L5)

Must run a `tokio::select!` loop: stdin for user input + `SignalBus` receiver for gate notifications (ADR-008). On `Signal::GateRequested`, print the prompt. On user `approve`/`reject`, send on the gate's `oneshot`.

---

## Consequences

### What becomes easier

- The Runner is simpler. No gate-specific logic. One terminal condition: `all_closed()`.
- The mental model is uniform: all gaps are just async tasks. Some await network I/O, some await human I/O. The JoinSet doesn't care.
- Human gets notified instantly — no waiting for unrelated gaps.
- Adding new HITL triggers (Executor, MicroAgent, Orchestrator) requires zero Runner changes. Just call `insert_gate()` and await the response.

### What becomes harder

- The JoinSet is now persistent across rounds (not recreated each loop). Need to be careful about its growth — though gaps always terminate eventually.
- The per-gap `oneshot` channel needs to be stored alongside the gate. `gates: DashMap<Uuid, (Value, oneshot::Sender<bool>)>` or a `Gate` struct.
- The CLI must be async-aware: `tokio::select!` over stdin + broadcast.

### What we'll need to revisit

- Gate timeout: if the user walks away, should gated gaps eventually time out? Probably yes — `tokio::time::timeout` wrapping the `oneshot::recv()`.
- Whether `Gated` state is even needed, or if `Assigned` with a flag is sufficient. Keeping `Gated` is cleaner for observability but adds a state that the Runner ignores.
- Gate payload structure: currently `Value`. As HITL requests diversify (approval vs. choice vs. text input), a structured enum may be needed.

---

## Action Items

1. [ ] **Build `SignalBus` (ADR-008).** `signal.rs` with `Signal` enum + `SignalBus` struct. Thread through `Moss` → `Orchestrator` → `Blackboard`. Blackboard mutations auto-emit.
2. [ ] **Update `insert_gate()`.** Emits `Signal::GateRequested`, stores `oneshot::Sender`, returns `oneshot::Receiver<bool>`.
3. [ ] **Rewrite Runner loop.** Persistent JoinSet. Process one completion per iteration. Terminal condition: `all_closed()` when JoinSet is empty. Remove `all_gated_or_closed()`.
4. [ ] **Remove `all_gated_or_closed()` from Blackboard.**
5. [ ] **Build `defense_claw.rs`.** Four-stage scan pipeline. Unit test each stage.
6. [ ] **Wire DefenseClaw into gap task.** Between compile and execute. On `Gated`: set state, insert gate, await oneshot. On `Rejected`: post failure, close.
7. [ ] **Update CLI to `tokio::select!` loop.** Subscribe to `SignalBus`. Show gate prompts on `Signal::GateRequested`. Handle `approve`/`reject` → send on oneshot.
