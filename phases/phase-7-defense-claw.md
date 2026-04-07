# Phase 7 — DefenseClaw

**Status:** Blocked on Phase 6
**ADRs:** [ADR-007](../docs/ADR-007-defenseclaw-and-hitl-gating.md)
**Effort:** ~100 lines. One focused session.

---

## Spec

Security scanner inserted between compile and execute in the Runner's per-gap task.

```rust
pub(crate) struct DefenseClaw {
    blocklist: Vec<String>,
    max_script_size: usize,
}

pub(crate) enum ScanVerdict {
    Approved,
    Gated { reason: String },
    Rejected { reason: String },
}

impl DefenseClaw {
    pub(crate) fn scan(&self, artifact: &Artifact, constraints: &Option<Value>) -> ScanVerdict;
}
```

**Four-stage pipeline (first non-Approved verdict wins):**

| Stage | Check | Verdict |
|-------|-------|---------|
| 1. Static analysis | Forbidden imports, network calls in Proactive scripts, writes outside sandbox | Rejected |
| 2. Capability check | Artifact tools vs. Gap constraints | Rejected |
| 3. Resource bounds | Timeout and memory limits set | Rejected |
| 4. HITL gate | High-risk action patterns (email, delete, purchase) against blocklist | Gated |

**Wiring into Runner (per-gap task):**

```rust
let artifact = compiler.compile(&gap, &prior).await?;

match defense_claw.scan(&artifact, gap.constraints()) {
    ScanVerdict::Approved => { /* fall through to execute */ }
    ScanVerdict::Gated { reason } => {
        bb.set_gap_state(&gap.gap_id(), GapState::Gated)?;
        let rx = bb.insert_gate(gap.gap_id(), reason);  // emits Signal::GateRequested
        let approved = rx.await.unwrap_or(false);         // await human I/O
        if !approved {
            // post rejection evidence, close gap
            return Ok(());
        }
        bb.set_gap_state(&gap.gap_id(), GapState::Assigned)?;
    }
    ScanVerdict::Rejected { reason } => {
        // post failure evidence, close gap
        return Ok(());
    }
}

Executor::new().run(&gap, &artifact, &bb).await?;
```

**Files:** `src/moss/defense_claw.rs` (new), `src/moss/runner.rs`, `src/moss/mod.rs`

**Tests:** Unit test each scan stage independently. Integration test: mock provider producing one gatable + one clean artifact, verify gate fires while clean runs.
