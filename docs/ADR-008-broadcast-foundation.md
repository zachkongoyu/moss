# ADR-008: Moss Broadcast Foundation

**Status:** Proposed
**Date:** 2026-04-07
**Deciders:** Zach (architect), contributors
**Relates to:** ADR-007 (HITL gating — first consumer), ARCHITECTURE.md §L3 (change notifications), §L5 (HUD)

---

## Context

Moss needs a way for any component to emit events and any other component to receive them — without the producer knowing who's listening or the consumer knowing who's emitting. Today there's no such mechanism. Components communicate through the Blackboard's data structures (DashMap reads/writes), which is pull-based: you have to poll to discover changes.

This matters right now because HITL gating (ADR-007) needs instant notifications — a gated gap must surface to the user the moment it happens, not on the next poll cycle. But the need is broader than gates:

- **HUD streaming** — gap state transitions, evidence arrivals, progress updates
- **CLI notifications** — gate prompts, error alerts, completion signals
- **Logging/audit** — structured event stream for observability
- **Future features** — popups, webhooks, plugin notifications, inter-session messaging

Building these as separate channels per feature creates a mess of wiring. Building one generic broadcast foundation means every feature just plugs in.

### Design constraints

- **Single-process.** Moss runs as one Tokio runtime. No network transport needed.
- **Multi-producer, multi-consumer.** Any component can emit. Multiple listeners can subscribe independently.
- **Non-blocking for producers.** Emitting an event must never block the emitter — even if consumers are slow or absent.
- **Typed but extensible.** Events should be structured (not stringly-typed), but adding new event kinds shouldn't require changing existing consumers.
- **Cheaply cloneable.** Events are broadcast to N consumers, so they must be `Clone`. Prefer small payloads or `Arc`-wrapped large ones.

---

## Decision

### The `Signal` — Moss's event bus

A single, system-wide broadcast channel. Any component with a handle to the `SignalBus` can emit or subscribe. The bus carries `Signal` values — a flat enum of everything that can happen in Moss.

```rust
// src/moss/signal.rs

use serde_json::Value;
use uuid::Uuid;
use tokio::sync::broadcast;

/// A system-wide event. Every variant is self-contained — no references,
/// no lifetimes. Cheap to clone (Box<str> and Uuid are small).
#[derive(Debug, Clone)]
pub enum Signal {
    /// A gap changed state.
    GapStateChanged {
        gap_id: Uuid,
        gap_name: Box<str>,
        old_state: Box<str>,
        new_state: Box<str>,
    },

    /// A gap needs human action. Carries context for the CLI/HUD to render.
    GateRequested {
        gap_id: Uuid,
        gap_name: Box<str>,
        reason: Box<str>,
    },

    /// A gate was resolved by the user.
    GateResolved {
        gap_id: Uuid,
        approved: bool,
    },

    /// Evidence posted for a gap.
    EvidencePosted {
        gap_id: Uuid,
        gap_name: Box<str>,
        status: Box<str>,   // "Success", "Failure", "Partial"
    },

    /// Blackboard intent updated.
    IntentUpdated {
        intent: Box<str>,
    },

    /// A new gap was inserted into the Blackboard.
    GapInserted {
        gap_id: Uuid,
        gap_name: Box<str>,
    },

    /// Blackboard sealed (topic change or session end).
    BoardSealed,

    /// Free-form system message (errors, warnings, info).
    /// Escape hatch for events that don't warrant a dedicated variant yet.
    System {
        level: SignalLevel,
        message: Box<str>,
    },
}

#[derive(Debug, Clone)]
pub enum SignalLevel {
    Info,
    Warn,
    Error,
}
```

### The `SignalBus` — owned once, shared everywhere

```rust
/// The broadcast backbone. Created once at startup. Passed (as Arc) to
/// every component that needs to emit or subscribe.
#[derive(Debug, Clone)]
pub struct SignalBus {
    tx: broadcast::Sender<Signal>,
}

impl SignalBus {
    /// Create a new bus with the given channel capacity.
    /// 64 is a sane default — events are small and consumed fast.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emit a signal. Non-blocking. If no subscribers exist, the signal
    /// is silently dropped — this is intentional. Producers should never
    /// care whether anyone is listening.
    pub fn emit(&self, signal: Signal) {
        let _ = self.tx.send(signal);  // ignore SendError (no receivers)
    }

    /// Subscribe to the bus. Returns a Receiver that yields every signal
    /// emitted after this point. Each subscriber gets its own independent
    /// stream — one slow subscriber doesn't affect others.
    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }
}
```

### Wiring — who gets the bus

`SignalBus` is created once in `Moss::new()` and threaded through to every component that needs it:

```rust
// src/lib.rs
pub struct Moss {
    orchestrator: Orchestrator,
    bus: SignalBus,  // owned here, cloned to components
}

impl Moss {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        let bus = SignalBus::new(64);
        Self {
            orchestrator: Orchestrator::new(provider, bus.clone()),
            bus,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.bus.subscribe()
    }
}
```

Components that emit:

| Component | Signals emitted | How it gets the bus |
|-----------|----------------|---------------------|
| **Blackboard** | `GapStateChanged`, `GapInserted`, `EvidencePosted`, `IntentUpdated`, `BoardSealed`, `GateRequested` | `Blackboard::new(bus)` — stored as field |
| **Runner** | (none directly — Blackboard emits on state changes) | Doesn't need it if Blackboard handles emission |
| **DefenseClaw** | (none directly — calls `bb.insert_gate()` which emits) | Doesn't need it |
| **Executor** | `System` (for errors/warnings during execution) | Passed to `Executor::new(bus)` if needed, or just use Blackboard |
| **Orchestrator** | `IntentUpdated` (on decompose), `BoardSealed` (on topic change) | `Orchestrator::new(provider, bus)` |

Components that subscribe:

| Component | Signals consumed | Notes |
|-----------|-----------------|-------|
| **CLI** | `GateRequested`, `GateResolved`, `System` | `tokio::select!` over stdin + bus |
| **HUD** | All signals | Renders delta stream to terminal |
| **Logger** | All signals (optional) | Structured event log |
| **Future** | Whatever they need | Just call `bus.subscribe()` and filter |

### Blackboard integration

The Blackboard gains a `SignalBus` field. Its mutation methods emit signals automatically — callers don't need to remember:

```rust
impl Blackboard {
    pub(crate) fn new(bus: SignalBus) -> Self {
        Self {
            bus,
            intent: Mutex::new(None),
            gaps: DashMap::new(),
            name_index: DashMap::new(),
            evidences: DashMap::new(),
            gates: DashMap::new(),
        }
    }

    pub(crate) fn set_gap_state(&self, gap_id: &Uuid, state: GapState) -> Result<(), MossError> {
        let mut entry = self.gaps.get_mut(gap_id)
            .ok_or_else(|| MossError::Blackboard(format!("gap {gap_id} not found")))?;
        let old = entry.state.clone();
        entry.state = state.clone();
        let name = entry.name.clone();

        self.bus.emit(Signal::GapStateChanged {
            gap_id: *gap_id,
            gap_name: name,
            old_state: format!("{old:?}").into_boxed_str(),
            new_state: format!("{state:?}").into_boxed_str(),
        });

        Ok(())
    }

    pub(crate) fn insert_gate(
        &self,
        gap_id: Uuid,
        reason: impl Into<Box<str>>,
    ) -> oneshot::Receiver<bool> {
        let reason: Box<str> = reason.into();
        let (tx, rx) = oneshot::channel();

        self.gates.insert(gap_id, tx);

        let gap_name = self.gaps.get(&gap_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "unknown".into());

        self.bus.emit(Signal::GateRequested {
            gap_id,
            gap_name,
            reason,
        });

        rx  // caller awaits this — just I/O
    }

    // ... insert_gap, append_evidence, set_intent all emit similarly ...
}
```

### Consumer pattern — CLI example

```rust
// main.rs — after creating Moss
let mut rx = moss.subscribe();

loop {
    tokio::select! {
        line = stdin.next_line() => {
            match line {
                Ok(Some(input)) => { /* handle user input, approve/reject, queries */ }
                _ => break,
            }
        }
        signal = rx.recv() => {
            match signal {
                Ok(Signal::GateRequested { gap_name, reason, .. }) => {
                    println!("[moss] '{gap_name}' needs your action: {reason}");
                    println!("       approve {gap_name} / reject {gap_name}");
                }
                Ok(Signal::System { level: SignalLevel::Error, message, .. }) => {
                    eprintln!("[moss] error: {message}");
                }
                // ignore signals we don't care about
                _ => {}
            }
        }
    }
}
```

Any future consumer follows the same pattern: `subscribe()`, `select!` or `while let`, match on the variants you care about, ignore the rest.

---

## Why not...

### Why not multiple typed channels?

One channel per event type (gate channel, evidence channel, state channel) gives you type safety per consumer — but the wiring is a nightmare. Every new event type means a new channel, a new field on every component that produces it, a new subscription for every consumer. With one bus, adding a new `Signal` variant is a one-line change. Consumers that don't care about it just ignore it via `_ => {}`.

### Why not `mpsc` instead of `broadcast`?

`mpsc` is single-consumer. We need multiple consumers (CLI + HUD + logger + future features) each getting every event independently. `broadcast` gives us that. The tradeoff: `broadcast` clones each event per subscriber. Since `Signal` is small (`Box<str>` + `Uuid` — under 100 bytes), cloning is cheap.

### Why not a trait-based observer pattern?

`trait SignalHandler { fn handle(&self, signal: &Signal); }` with dynamic dispatch. More flexible in theory, but adds complexity (registration, lifetime management, dynamic dispatch overhead) for no practical gain in a single-process Tokio app where `broadcast` already does exactly this.

### Why not embed the bus in the Blackboard only?

Because not all signals originate from the Blackboard. The Orchestrator emits `BoardSealed` on topic change. The Executor might emit `System` warnings. Future components (MCP bridge, memory tiers) will have their own events. The bus is system-level, not Blackboard-level.

---

## Consequences

### What becomes easier

- **Adding new event types.** Add a variant to `Signal`. Emitter calls `bus.emit()`. Done. No wiring changes.
- **Adding new consumers.** Call `bus.subscribe()`. Match on what you care about. Done.
- **HITL gating (ADR-007).** `insert_gate()` emits `GateRequested`, CLI picks it up instantly. The oneshot for the per-gate response is orthogonal to the bus — the bus broadcasts the notification, the oneshot carries the reply.
- **HUD.** Subscribe to the bus, render every signal as a terminal delta. The bus already carries everything the HUD needs.
- **Testing.** Subscribe in tests, assert that expected signals were emitted. No mocking infrastructure needed.

### What becomes harder

- **Every mutation method on Blackboard now has a side effect** (emitting a signal). This is the right tradeoff — the alternative is requiring every caller to remember to emit, which they won't.
- **Signal is a flat enum that will grow.** As Moss gains features, `Signal` gains variants. This is fine — Rust's exhaustive matching warns you if you forget to handle a new variant (in matches without `_ =>`). And consumers that use `_ =>` explicitly opt out of caring.
- **Broadcast has the lagging receiver problem.** If a subscriber falls behind by more than `capacity` events, it gets a `RecvError::Lagged(n)`. Mitigation: use a reasonable buffer (64), and consumers should be fast (non-blocking renders, not heavy computation).

### What we'll need to revisit

- **Whether `Signal` should carry richer payloads.** Currently it uses `Box<str>` for state names (e.g., `"Ready"`, `"Closed"`). Could use the actual enum types, but that couples `Signal` to `GapState` at the type level. `Box<str>` keeps it decoupled. Revisit if the stringly-typed approach causes bugs.
- **Whether some signals need guaranteed delivery.** `broadcast` drops events if no subscribers exist. For audit logging, this might matter. If so, a parallel `mpsc` to a dedicated logger task could supplement the bus — but that's a future concern.
- **Channel capacity tuning.** 64 is a guess. Under load with many parallel gaps, signals might spike. Monitor for `Lagged` errors in production and bump if needed.

---

## Action Items

1. [ ] **Create `src/moss/signal.rs`.** `Signal` enum, `SignalLevel`, `SignalBus` struct with `new()`, `emit()`, `subscribe()`.
2. [ ] **Add `SignalBus` to `Blackboard`.** Constructor takes `SignalBus`. Mutation methods (`set_gap_state`, `insert_gap`, `append_evidence`, `set_intent`, `insert_gate`) emit appropriate signals.
3. [ ] **Update `insert_gate()` signature.** Returns `oneshot::Receiver<bool>`. Stores `oneshot::Sender` in the gates map. Emits `GateRequested`.
4. [ ] **Thread `SignalBus` through `Moss` → `Orchestrator` → `Blackboard`.** `Moss::new()` creates the bus. `Moss::subscribe()` exposes it to external consumers (CLI).
5. [ ] **Update `Blackboard::new()` call sites** to pass the bus.
6. [ ] **Unit tests.** Emit signals from Blackboard methods, subscribe in test, assert correct signals received. Test no-subscriber case (silent drop). Test lagged receiver handling.
7. [ ] **Wire CLI.** `main.rs` subscribes to bus, uses `tokio::select!` over stdin + receiver. Initially just print `GateRequested` and `System` signals. Other signals ignored for now.
