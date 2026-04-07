# Moss AIOS — Architecture Specification

**Version:** 0.7.0
**Date:** 2026-04-07
**Status:** Living document — each component is marked with its implementation status.

**Status legend:**

| Tag | Meaning |
|-----|---------|
| `IMPLEMENTED` | Code exists, compiles, and is exercised by at least one path |
| `PARTIAL` | Skeleton or stub exists; core logic incomplete |
| `PLANNED` | Designed but no code yet |

---

## 1. Overview

Moss is a local-first AI Operating System that transforms a single user intent into a parallel execution plan, runs it, and synthesizes a result. It is built in Rust on Tokio and uses one or more LLM providers for reasoning.

The system follows the **Blackboard architecture pattern** (Hearsay-II lineage): independent specialist components read from and write to a shared, structured memory space (the Blackboard), coordinated by a central Orchestrator that decomposes intent into a Directed Acyclic Graph (DAG) of atomic tasks called Gaps.

### 1.1 Design Principles

1. **Living Blackboard.** A Blackboard is a workspace, not a transaction. It stays open across follow-up messages: the Orchestrator appends new Gaps and refines the intent as the conversation evolves. The Gap array only grows — Gaps are never removed. A new Blackboard is created only when the Orchestrator determines the user's query is unrelated to the current workspace, or when the session ends.
2. **Code as the universal solver.** Every Gap is resolved by generating and executing code (a deterministic script or a reactive agent loop), not by prompting the LLM to "think harder."
3. **Failure containment.** A failing Gap does not corrupt the global Blackboard. Reactive tasks run inside encapsulated Micro-Agent instances running an isolated ReAct loop.
4. **Concurrency by default.** Independent Gaps execute in parallel via `tokio::JoinSet`. The DAG structure — not a global lock — determines ordering.
5. **Defense in depth.** All generated artifacts pass through `ArtifactGuard` before execution.

---

## 2. System Layers

```
L5  Interface          CLI daemon, HUD delta streamer
L4  Orchestrator       Intent decomposition, DAG management, drive_gaps, response synthesis
L3  Blackboard         Living workspace: Gaps (append-only), Evidence, mutable intent, HITL approvals
L2  Compiler/Executor  Gap-to-artifact compilation, sandboxed execution
L1  Memory             Session context (M1), local DB (M2), vector store (M3)
L0  Infrastructure     LLM providers, MCP bridge, ArtifactGuard scanner
```

### Layer responsibilities

**L5 — Interface** `PARTIAL`
The user-facing surface. `Cli` struct in `src/cli.rs` drives a `tokio::select!` loop over stdin + signal bus. The planned HUD component subscribes to the same signal bus for real-time delta streaming.

| Sub-component | Status | Notes |
|---|---|---|
| CLI input loop | `IMPLEMENTED` | `src/cli.rs` — `Cli` struct, async stdin reader, calls `Moss::run`, prints response. |
| CLI signal handling | `IMPLEMENTED` | `src/cli.rs` — inner `tokio::select!` over stdin + signal receiver. Surfaces `ApprovalRequested` events inline, prompts `[y/N]`, calls `Moss::approve()`. |
| HUD delta streamer | `PLANNED` | Another signal bus consumer. Phase 11. |

**L4 — Orchestrator** `IMPLEMENTED`
The strategic coordinator. `Orchestrator::run` is the single entry point: decompose → insert Gaps → `drive_gaps` (private JoinSet loop) → synthesize. Owns the current `Arc<Blackboard>` and the broadcast sender.

| Sub-component | Status | Notes |
|---|---|---|
| Intent-to-DAG decomposition (single LLM call) | `IMPLEMENTED` | `orchestrator.rs` — `decompose` renders `prompts/decompose.md` via `minijinja`, calls LLM, deserializes into `Decomposition` (includes `is_follow_up` flag). |
| Response synthesis (Evidence → answer) | `IMPLEMENTED` | `orchestrator.rs` — `synthesize` renders `prompts/synthesize.md`, passes real evidence from `blackboard.all_evidence()`. |
| Execution loop (poll, dispatch, evidence, synthesis) | `IMPLEMENTED` | `orchestrator.rs` — `Orchestrator::drive_gaps()` private method. `runner.rs` dissolved. JoinSet fan-out, `MAX_RETRIES=3`, deadlock detection. |
| Context injection (M1/M3 retrieval before planning) | `PLANNED` | — |

**L3 — Blackboard** `PARTIAL`
Living workspace using `DashMap` for lock-free concurrent access. Holds the intent (mutable), the Gap DAG (append-only), accumulated Evidence, and human-in-the-loop Gates. A Blackboard stays open across follow-up messages — new Gaps are inserted and the intent is refined on each decompose call. It is sealed only when the topic changes or the session ends (see Section 10).

| Sub-component | Status | Notes |
|---|---|---|
| Data structures (Gap, Evidence, Blackboard) | `IMPLEMENTED` | `blackboard.rs` — `GapState`, `GapType`, `Gap`, `EvidenceStatus`, `Evidence`, `Blackboard` with private fields and `pub(crate)` getters |
| Insert/mutate operations | `IMPLEMENTED` | `insert_gap`, `set_gap_state`, `append_evidence`, `set_intent`, `register_approval`, `approve` |
| Dependency resolution (auto-unblock) | `IMPLEMENTED` | `promote_unblocked`, `drain_ready`, `all_closed` — unit tested. `all_gated_or_closed` removed (ADR-007). |
| Signal Bus integration | `IMPLEMENTED` | Every `Blackboard` mutation emits `Event::Snapshot` via broadcast. `Orchestrator` emits `Event::ApprovalRequested` on HITL gate. `register_approval()`/`approve()` manage the `oneshot` pair. |

**L2 — Compiler & Executor** `IMPLEMENTED`
The Compiler takes a Gap description and emits an executable artifact. The Executor runs it and posts Evidence back to the Blackboard. The execution loop (`drive_gaps`) lives on the Orchestrator.

| Sub-component | Status | Notes |
|---|---|---|
| Compiler | `IMPLEMENTED` | `compiler.rs` — renders `prompts/compiler.md`, calls LLM, deserializes into `Artifact` (Script or Agent). Language-agnostic. |
| Executor — script runner | `IMPLEMENTED` | `executor.rs` — zero-size unit struct. Writes code to `NamedTempFile`, spawns interpreter via `tokio::process::Command`, bounded by `tokio::time::timeout`. Writes Evidence to Blackboard. |
| Execution loop | `IMPLEMENTED` | Absorbed into `Orchestrator::drive_gaps()` — `runner.rs` deleted. |
| Executor — Micro-Agent host (ReAct loop) | `PLANNED` | Stub in `executor.rs` — writes Failure evidence. Phase 8. |
| Sandbox / isolation | `PLANNED` | — |

**L1 — Memory** `PLANNED`
Three-tier memory hierarchy for context across and within sessions.

| Tier | Store | Purpose | Status |
|---|---|---|---|
| M1 | In-process session context | Cross-board awareness: sealed board summaries, key entities | `PLANNED — design open` |
| M2 | Sled (embedded KV) | User preferences, audit trail | `PLANNED` — not in `Cargo.toml` |
| M3 | Qdrant (vector DB) | Knowledge Crystals — compressed outcomes from past sessions | `PLANNED` — not in `Cargo.toml` |

**L0 — Infrastructure** `PARTIAL`

| Sub-component | Status | Notes |
|---|---|---|
| Provider trait + OpenRouter impl | `IMPLEMENTED` | `providers/` — working against OpenRouter API |
| Local mock provider | `IMPLEMENTED` | `providers/local/mod.rs` |
| MCP client (tool bridge) | `PLANNED` | See Section 7 |
| ArtifactGuard (pre-exec scanner) | `IMPLEMENTED` | `artifact_guard.rs` — zero-field struct, 4-stage scan pipeline, `HITL_PATTERNS` const. See Section 8. |

---

## 3. Core Runtime Loop

This is the central execution flow that ties L4, L3, and L2 together. It is called once per user message. The same Blackboard may pass through this loop many times across follow-up messages.

`IMPLEMENTED` — `Moss::run` → `Orchestrator::run` → `decompose` → gap insertion → `drive_gaps` (Compiler → ArtifactGuard → Executor per gap, including HITL round-trip) → `synthesize`. Full end-to-end loop implemented and running.

### 3.1 Sequence

```
User input
    |
    v
[1] Moss: is there an active Blackboard?
    |
    YES                             NO
    |                               |
    v                               v
[2] Serialize board state       Create new Blackboard
    (blackboard.snapshot())
    |                               |
    +---------------+---------------+
                    |
                    v
[3] Orchestrator.decompose(query, blackboard)
    LLM returns: { intent, is_follow_up, gaps[] }
                    |
                    v
[4] Orchestrator: is_follow_up? (from Decomposition struct)
    |
    Follow-up                   New topic
    |                           |
    Update intent               Seal current board → M3
    on current board            Create new board, set intent
    |                           |
    +---------------+-----------+
                    |
                    v
[5] Insert new Gaps into Blackboard
    - New Gaps with deps on existing Closed Gaps → Ready immediately
    - New Gaps with deps on other new Gaps → Blocked
    promote_unblocked()
                    |
                    v
[6] EXECUTION LOOP (Orchestrator::drive_gaps — persistent JoinSet, one completion per iteration):
    |
    |   6a. promote_unblocked() — check Blocked Gaps
    |   6b. drain_ready() — poll Blackboard for all Ready Gaps
    |   6c. For each Ready Gap, spawn into tokio::JoinSet:
    |       6c-i.    Mark Gap as Assigned
    |       6c-ii.   Send Gap to Compiler (LLM call)
    |       6c-iii.  Compiler returns artifact (Script or AgentSpec)
    |       6c-iv.   ArtifactGuard scans artifact
    |       6c-v.    If Gated: Gap → Gated, insert Gate (fires broadcast to CLI),
    |                await human response on oneshot channel (this is just I/O —
    |                the task stays on the JoinSet like any other async wait)
    |       6c-vi.   If Rejected: post Failure evidence, Gap → Closed, return
    |       6c-vii.  Executor runs artifact
    |       6c-viii.  Executor posts Evidence to Blackboard
    |       6c-ix.   Gap → Closed
    |   6d. If JoinSet is empty:
    |       - all Gaps Closed → done
    |       - else → deadlock
    |   6e. Wait for ONE task to complete (join_next), then loop back to 6a
    |
    |   Note: Gated gaps stay alive on the JoinSet awaiting human I/O.
    |   Other gaps complete, promote dependents, and get dispatched
    |   without waiting for the human. drive_gaps has no gate-specific logic.
    |
    v
[7] Orchestrator.synthesize() — reads latest intent + all Evidence
    |
    v
[8] Return response to L5. Blackboard enters Idle state.
    (Board stays in memory — ready for follow-up on next input)
```

### 3.2 Concurrency constraints

- **Fan-out limit.** A `tokio::Semaphore` caps the number of concurrently executing Gaps (default: 4). This bounds LLM call parallelism and subprocess count.
- **No mutable aliasing.** The `Blackboard` is behind `Arc` and uses `DashMap` internally, so concurrent readers/writers do not require a mutex. Gap state transitions are atomic per-entry.
- **Deadlock detection.** If the JoinSet drains to empty but Blocked gaps remain, the loop returns a `Deadlock` error rather than hanging. This can happen if the Orchestrator produces a DAG with a cycle or an unresolvable dependency.

---

## 4. Component Specifications

### 4.1 Orchestrator `IMPLEMENTED`

**Responsibility:** Translate user intent into new Gaps; refine the Blackboard's intent on follow-ups; synthesize the final response from Evidence.

**Current state:** Full execution pipeline implemented. `Orchestrator::run` is the single entry point: it calls `decompose`, inserts Gaps, calls `drive_gaps` (private JoinSet execution loop with ArtifactGuard + HITL), and calls `synthesize`. The Orchestrator owns a `Mutex<Arc<Blackboard>>` for follow-up tracking and a `broadcast::Sender` for HITL signals. `Decomposition::is_follow_up` flag drives board reuse vs. fresh creation.

**Current interface (as-built):**

```rust
pub(crate) struct Orchestrator {
    provider: Arc<dyn Provider>,
    compiler: Arc<Compiler>,
    guard: Arc<ArtifactGuard>,
    blackboard: Mutex<Arc<Blackboard>>,
    tx: broadcast::Sender<signal::Payload>,
}

impl Orchestrator {
    pub(crate) fn new(provider: Arc<dyn Provider>, tx: broadcast::Sender<signal::Payload>) -> Self;
    pub(crate) fn approve(&self, gap_id: Uuid, approved: bool);
    pub(crate) async fn run(&self, query: &str) -> Result<String, MossError>;
    // private:
    async fn drive_gaps(&self, blackboard: Arc<Blackboard>) -> Result<(), MossError>;
    pub(crate) async fn decompose(&self, query: &str, blackboard: &Blackboard) -> Result<Decomposition, MossError>;
    pub(crate) async fn synthesize(&self, blackboard: &Blackboard) -> Result<String, MossError>;
}
```

**Decompose interface:** `decompose` receives `&Blackboard` directly and calls `blackboard.snapshot()` to serialize the current board state into the planning prompt. The `Decomposition` response includes `is_follow_up: bool` which the Orchestrator uses to decide whether to reuse the current board or create a fresh one.

**Decompose output contract:**

The LLM always returns `{ intent, is_follow_up, gaps[] }`. `is_follow_up` is the Orchestrator's decision on whether to extend the current board.

- `intent` — the current goal of the Blackboard. On the first message this is the original intent. On follow-ups the Orchestrator refines it to capture the evolved scope (e.g., "Book a flight to Tokyo" → "Book a business class flight to Tokyo"). Always present.
- `is_follow_up` — `true` if this query extends the current Blackboard; `false` if it starts a new topic. Decided by the LLM in the decompose call.
- `gaps[]` — only the **new** Gaps needed for this query. On a follow-up, these may declare dependencies on existing Closed Gaps by name. On a new topic, these will have no references to the current board.

```json
{
  "intent": "string — the current/updated goal",
  "is_follow_up": true,
  "gaps": [
    {
      "name": "snake_case_identifier (unique across board lifetime)",
      "description": "what this gap resolves",
      "gap_type": "Proactive | Reactive",
      "dependencies": ["may reference existing Closed gaps or new gaps"],
      "constraints": null,
      "expected_output": "what a correct result looks like"
    }
  ]
}
```

**Rich Blackboard state (input to decompose):**

`Blackboard::snapshot()` serializes the board into a `BlackboardSnapshot` struct (intent + all Gaps + all Evidence) that is rendered into the planning prompt via minijinja:

```json
{
  "intent": "Book a flight to Tokyo",
  "gaps": [
    {
      "name": "search_flights",
      "state": "Closed",
      "description": "Search for available flights to Tokyo",
      "evidence_summary": { "found": 12, "cheapest": "$450" }
    }
  ]
}
```

For the first message in a session (no board exists), this is `{}`.

**Prompt contract:**
- `prompts/decompose.md` — Markdown instructions + XML-tagged input (`{{ user_query }}`, `{{ blackboard_state }}`). The prompt instructs the LLM to refine the intent on follow-ups, return only new Gaps, and avoid reusing names already on the board. See Section 10 for the full lifecycle.
- `prompts/synthesize.md` — Markdown instructions + XML-tagged input (`{{ intent }}`, `{{ evidence }}`). The `intent` is always the latest (refined) version. LLM returns a plain text response.

### 4.2 Blackboard `IMPLEMENTED`

**Responsibility:** Living workspace for the current conversation thread. Holds the intent (mutable), the Gap DAG (append-only), Evidence map, and HITL Gates. A Blackboard stays open across follow-up messages — the Orchestrator inserts new Gaps and updates the intent on each decompose call. It is sealed only when the topic changes or the session ends (see Section 10).

**Current state:** Core data structures, insert/mutate operations, dependency resolution, ready-gap polling, signal bus integration, and HITL approval flow are implemented and unit tested.

**Implemented interface:**

```rust
impl Blackboard {
    /// Return and atomically mark as Assigned all gaps currently in Ready state.
    pub(crate) fn drain_ready(&self) -> Vec<Gap>;

    /// For every Blocked gap whose dependencies are all Closed, promote to Ready.
    pub(crate) fn promote_unblocked(&self);

    /// True when every gap is in Closed state. This is the execution loop's only terminal condition.
    pub(crate) fn all_closed(&self) -> bool;

    /// Retrieve a gap by ID (cloned for send across await).
    pub(crate) fn get_gap(&self, id: &Uuid) -> Option<Gap>;

    /// Retrieve a gap UUID by name slug. Used by promote_unblocked and dependency resolution.
    pub(crate) fn get_gap_id_by_name(&self, name: &str) -> Option<Uuid>;

    /// Store the sender half of a HITL oneshot channel.
    /// The Orchestrator emits ApprovalRequested then awaits the receiver side.
    pub(crate) fn register_approval(&self, gap_id: Uuid, sender: oneshot::Sender<bool>);

    /// Resolve a pending approval. Called by Moss::approve() from the CLI.
    pub(crate) fn approve(&self, gap_id: Uuid, approved: bool);

    /// Access the broadcast sender to emit events from outside the Blackboard.
    pub(crate) fn signal_tx(&self) -> &broadcast::Sender<signal::Payload>;
}
```

All Blackboard mutation methods (`set_gap_state`, `insert_gap`, `append_evidence`, `set_intent`) emit `Event::Snapshot` via the broadcast channel after each write. Consumers (CLI, HUD, logger) subscribe independently — the Blackboard doesn't know or care who's listening.

**Removed:** `all_gated_or_closed()` — no longer needed. The execution loop's terminal condition is `all_closed()`. Gated gaps stay alive on the JoinSet as async I/O waits; `drive_gaps` has no gate-specific logic (see ADR-005, ADR-007).

**Removed:** `subscribe()` from Blackboard — subscription is now on `Moss::subscribe()` (ADR-008).

**Removed:** `insert_gate()` — replaced by `register_approval()` + `approve()`. The Orchestrator creates the `oneshot` pair directly and emits `Event::ApprovalRequested` via `signal_tx()`.

**Name→UUID reverse index:**
`Gap.dependencies` stores names (`Vec<Box<str>>`), but the gap map is keyed by `Uuid`. A secondary index `name_index: DashMap<Box<str>, Uuid>` is populated atomically in `insert_gap` alongside the primary map. This makes `promote_unblocked` O(D) per gap (D = dependency count) instead of O(N·D) with a scan. The index is append-only — gap names are immutable after insertion.

**Data structures — as implemented:**

`blackboard.rs` implements the full target design. All struct fields are private; access is via `pub(crate)` getters. Types are `pub(crate)`. The structs are:

```rust
// All fields private; access via pub(crate) getters only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Gap {
    gap_id: Uuid,
    name: Box<str>,              // snake_case slug from the plan
    state: GapState,
    description: Box<str>,       // consumed by the Compiler
    gap_type: GapType,           // Proactive or Reactive
    dependencies: Vec<Box<str>>, // names of gaps this depends on
    constraints: Option<Value>,
    expected_output: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum GapType {
    Proactive,
    Reactive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Evidence {
    gap_id: Uuid,
    attempt: u32,            // 1-based attempt number (for retry history)
    content: Value,
    status: EvidenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum EvidenceStatus {
    Success,
    Failure { reason: String },
    Partial,                     // Micro-Agent hit iteration cap before goal was met
}
```

`Blackboard.evidences` is `DashMap<Uuid, Vec<Evidence>>` — an ordered attempt log per gap. The Compiler for retry attempt N receives the `Vec<Evidence>` slice `[0..N-1]` so it can see prior errors and adapt.

`Blackboard` includes `name_index: DashMap<Box<str>, Uuid>` for O(1) name-to-ID resolution. Written once in `insert_gap`, never mutated after that. `intent` is stored as `Mutex<Option<Box<str>>>` for safe mutation through a shared `&self` reference. `pending_approvals: DashMap<Uuid, oneshot::Sender<bool>>` stores the sender half of each HITL oneshot channel until `approve()` is called.

**Thread-safety model:**
`DashMap` provides per-shard read/write locks internally. Individual Gap state transitions (Ready -> Assigned) must be atomic. Use `DashMap::get_mut` which holds a write lock on the shard for the duration of the returned `RefMut`. The `drain_ready` method should iterate and CAS (compare-and-swap) in a single pass to avoid TOCTOU races where two threads both see the same gap as Ready.

### 4.3 Compiler `IMPLEMENTED`

**Responsibility:** Accept a Gap description and emit an executable artifact — either a self-contained script (Proactive) or a Micro-Agent specification (Reactive).

**Interface:**

```rust
pub(crate) struct Compiler {
    provider: Arc<dyn Provider>,
}

#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Artifact {
    Script {
        language: Box<str>,  // "python3", "bash", "node", etc.
        code: Box<str>,
        timeout_secs: u64,
    },
    Agent {
        role: Box<str>,
        goal: Box<str>,
        tools: Vec<Box<str>>,  // MCP tool names
        instructions: Box<str>,
        // max_iterations and timeout are PLANNED — Agent execution is a stub
    },
}

impl Compiler {
    /// Compile a Gap into an executable Artifact.
    /// `prior_attempts` contains error reason strings from previous failed runs (empty on first attempt).
    pub(crate) async fn compile(&self, gap: &Gap, prior_attempts: &[Box<str>]) -> Result<Artifact, MossError>;
}
```

**Prompt contract (`compiler.md`):**
The Compiler prompt receives the gap name, description, type, and prior error strings. It returns a JSON object with `type` (`SCRIPT` or `AGENT`) and the relevant payload fields (`language`, `code`, `timeout_secs` for scripts; `role`, `goal`, `tools`, `instructions` for agents).

**Design decisions:**
- The Compiler must not have access to the full Blackboard — only the specific Gap description and its resolved dependency Evidence. This enforces the principle of least privilege and keeps the LLM context window focused.
- Script artifacts are self-contained: they must include all imports, accept input via stdin or environment variables, and write output to stdout as JSON.
- Agent specs are declarative: they describe *what* the agent should achieve, not the exact steps. The Executor's agent runtime interprets the spec.

### 4.4 Executor `PARTIAL`

**Responsibility:** Run artifacts in isolation and produce Evidence. Script path is fully implemented; Agent path is a stub.

**Interface:**

```rust
/// Zero-size unit struct — stateless, no LLM, no shared state.
#[derive(Clone, Copy)]
pub(crate) struct Executor;

impl Executor {
    /// Run an artifact and write Evidence to the Blackboard directly.
    /// Script path: fully implemented. Agent path: stub — writes Failure evidence.
    pub(crate) async fn run(
        &self,
        gap: &Gap,
        artifact: &Artifact,
        blackboard: &Blackboard,
    ) -> Result<(), MossError>;
}
```

**Script execution model:**
1. Write the script to a temporary file inside a sandbox directory.
2. Spawn a child process (`tokio::process::Command`) with restricted environment: no network access for Proactive scripts (they receive all data via stdin), bounded CPU time via `timeout`, bounded memory via cgroups or ulimit.
3. Capture stdout as JSON. Parse into `Evidence.content`.
4. If the process exits non-zero or times out, return an error Evidence with the stderr content, and let the Orchestrator decide whether to retry or fail the gap.

**Micro-Agent execution model:**
1. The `Compiler` returns `Artifact::AgentSpec { goal, tools, instructions, max_iterations, timeout }`.
2. `Executor::run()` constructs `MicroAgent { goal, tools, max_iterations, provider, context }` where `context` is the read-only dependency Evidence passed in. No sub-Blackboard is created.
3. `MicroAgent::run()` executes a ReAct loop using an internal `Vec<Message>` as local scratch memory. This history never touches the parent Blackboard.
4. Each iteration: LLM call with tool definitions scoped to `tools` → LLM returns tool call or final answer → if tool call, invoke via `McpBridge` → append observation to history → check if goal is met.
5. On exit (goal met or `max_iterations` exhausted): serialize the final answer and key observations into a single `Evidence` record. Internal history is discarded. Evidence is returned to the Executor, which posts it to the parent Blackboard and marks the Gap Closed.

```rust
pub struct MicroAgent {
    goal: String,
    tools: Vec<String>,          // permitted MCP tool names only — least privilege
    max_iterations: u32,
    provider: Arc<dyn Provider>, // same provider pool, no new Orchestrator
    context: Vec<Evidence>,      // dependency Evidence — read-only input
    history: Vec<Message>,       // internal scratch — never written to Blackboard
}

impl MicroAgent {
    pub async fn run(mut self, mcp: &McpBridge) -> Result<Evidence>;
}
```

### 4.5 DAG Scheduler

The scheduler is not a separate component — it is `Orchestrator::drive_gaps` (Section 3). `runner.rs` was dissolved into the Orchestrator (Phase 7). This is a deliberate simplification: an external scheduler would add an inter-component communication layer without clear benefit at this scale.

**Scheduling strategy:** Non-preemptive, event-driven. Gaps are not assigned on a timer; they are spawned into the JoinSet when (a) they become Ready and (b) a semaphore permit is available. When a gap completes and posts Evidence, the `promote_unblocked` sweep runs synchronously before the next iteration, ensuring newly-unblocked gaps are immediately eligible.

**Failure policy:**

| Failure type | Behavior |
|---|---|
| Script exits non-zero | Retry up to N times (default 2). The error stderr is stored as `EvidenceStatus::Failure { reason }`. On each retry, `compiler.compile(gap, prior_attempts)` receives all prior failure records so it can adapt the generated code. After N failures, mark Gap as Closed with `EvidenceStatus::Failure` and propagate to dependents. |
| Micro-Agent exceeds iteration cap | Serialize partial history as a summary. Mark Closed with `EvidenceStatus::Partial`. |
| Micro-Agent exceeds timeout | Abort MicroAgent ReAct loop, collect partial history as Evidence. Mark Closed with `EvidenceStatus::Partial`. |
| LLM provider error (rate limit, timeout) | Exponential backoff with jitter, up to 3 retries. |
| Deadlock (Blocked gaps remain, no Ready/Assigned/Gated) | Return `MossError::Deadlock`. Log full DAG state. |

---

## 5. Memory Hierarchy `PLANNED`

### 5.1 M1 — Session Context `PLANNED — design open`

M1 provides session-level awareness across sealed Blackboards. When a Blackboard is sealed (topic change), the Orchestrator needs a lightweight summary of what happened on prior boards — not the full Evidence, but enough to know the session history.

The exact structure is TBD. Candidates include a list of per-board summaries (intent + outcome + key entities), a running entity map (names, preferences, references discovered during the session), or Crystal IDs for same-session boosting in M3 retrieval.

Note: within a single Blackboard, M1 is not needed — the Blackboard itself holds all Gaps, Evidence, and the current intent. M1 only matters for context that spans sealed Blackboard boundaries within the same session.

### 5.2 M2 — Sled (Local Preferences & Audit)

An embedded key-value store for data that must survive across sessions but does not need semantic search.

Contents: user preferences (default model, concurrency limits, tool permissions), an append-only audit log of all executed artifacts (for security review), and session metadata (start time, gap count, outcome).

**Dependency:** `sled` crate (to be added to `Cargo.toml`).

### 5.3 M3 — Qdrant (Knowledge Crystals)

A vector database for semantic retrieval of compressed past session outcomes.

**Crystallization trigger:** When a Blackboard is sealed (topic change or session end) and contains at least one Closed Gap with `EvidenceStatus::Success`, the system generates a Knowledge Crystal:

```rust
pub struct Crystal {
    session_id: Uuid,
    intent: String,
    outcome_summary: String, // LLM-compressed summary of all Evidence
    embedding: Vec<f32>,     // embedding of intent + outcome
    timestamp: DateTime<Utc>,
    tags: Vec<String>,       // extracted entities, tool names used
}
```

**Retrieval:** Before the decomposition step, the Orchestrator embeds the new query and retrieves the top-K (default 5) most similar Crystals from Qdrant. These are injected into the planning prompt as prior context, giving the system "memory" of how it solved similar problems before.

**Dependency:** `qdrant-client` crate (to be added to `Cargo.toml`).

---

## 6. Provider Abstraction `IMPLEMENTED`

The `Provider` trait abstracts LLM access behind a single async method:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete_chat(&self, messages: Vec<Message>) -> Result<String, ProviderError>;

    /// Default: returns `Err(ProviderError::NotSupported)`.
    /// Override in providers that support function/tool calling.
    async fn complete_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ToolCallOrText, ProviderError> {
        Err(ProviderError::NotSupported)
    }
}
```

**Current implementations:**

| Provider | Status | Notes |
|---|---|---|
| OpenRouter | `IMPLEMENTED` | Supports any model available via OpenRouter API |
| LocalMock | `IMPLEMENTED` | Echo-back mock for testing |
| Local vLLM | `PLANNED` | Direct inference on local GPU via vLLM's OpenAI-compatible API |

**Remaining work:**
- **Streaming.** `PLANNED` — For the HUD to stream partial responses, add `complete_chat_stream` returning a `Stream<Item = Result<String>>`.
- **Tool calling.** `PLANNED` — `complete_with_tools` stub exists; full implementation is required for the Micro-Agent's ReAct loop.

---

## 7. MCP Integration `PLANNED`

MCP (Model Context Protocol) is the standardized bridge between the LLM and external tools (filesystem, browser, APIs, databases).

**Design:**

```rust
pub struct McpBridge {
    servers: Vec<McpServerHandle>,
    tool_registry: HashMap<String, ToolDefinition>,
}

impl McpBridge {
    /// Discover all tools from connected MCP servers.
    pub async fn discover(&mut self) -> Result<()>;

    /// Invoke a tool by name with JSON arguments.
    pub async fn call(&self, tool_name: &str, args: Value) -> Result<Value>;

    /// Return tool definitions formatted for LLM function-calling.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition>;
}
```

**Transport:** JSON-RPC 2.0 over stdio (spawn MCP server as a child process and communicate via stdin/stdout). This is the standard MCP transport.

**Dependency:** `mcp-rust-sdk` or manual JSON-RPC implementation over `tokio::process::Command`.

**Tool scoping:** The Executor provides each Micro-Agent only the tools listed in its `AgentSpec.tools` field. This prevents a web-browsing agent from accessing the filesystem, and a file-management agent from making network calls.

---

## 8. Security: ArtifactGuard `IMPLEMENTED`

`ArtifactGuard` (`src/moss/artifact_guard.rs`) is the pre-execution scanner that inspects every artifact before the Executor runs it. It is a zero-field unit struct with all policy encoded as constants. The conceptual security layer is still called "DefenseClaw" in design discussions; the struct name in code is `ArtifactGuard`. It operates as a pipeline of checks, any of which can reject the artifact.

**Scan pipeline:**

| Stage | What it checks | Method |
|---|---|---|
| 1. Static analysis | Forbidden imports (`os.system`, `subprocess`, `shutil.rmtree`), network calls in Proactive scripts, filesystem writes outside sandbox | AST parsing (Python `ast` module via a small Python helper, or `tree-sitter` from Rust) |
| 2. Capability check | Does the artifact require capabilities beyond what the Gap's constraints allow? | Compare requested tool names against the Gap's permitted tool list |
| 3. Resource bounds | Are timeout and memory limits set? | Config validation |
| 4. HITL gate | Is this a high-risk action (e.g., sending email, deleting files, making purchases)? | Pattern match against `HITL_PATTERNS` const (pattern + category tuples); if matched, emit `Event::ApprovalRequested` and await `oneshot` response |

**Interface:**

```rust
/// Zero-field unit struct — all policy encoded as constants.
pub(crate) struct ArtifactGuard;

const MAX_SCRIPT_SIZE: usize = 65_536;

/// Pattern + category pairs for HITL gating. Category surfaces in the approval prompt.
/// Examples: ("> /dev/tcp", "network exfil"), ("stripe.charge(", "financial")
const HITL_PATTERNS: &[(&str, &str)] = &[ /* see artifact_guard.rs */ ];

/// A single scan pass produces one of three verdicts — never ambiguous.
pub(crate) enum ScanVerdict {
    /// Artifact is clean. Proceed to execution.
    Approved,
    /// High-risk action detected. Pause Gap, surface approval request to user.
    Gated   { reason: Box<str> },
    /// Hard violation (forbidden import, oversized script, etc.). Do not execute.
    Rejected { reason: Box<str> },
}

impl ArtifactGuard {
    pub(crate) fn new() -> ArtifactGuard;
    /// Run all 4 stages in one pass and return a single verdict.
    /// Callers dispatch on the variant — no two-method TOCTOU window.
    pub(crate) fn scan(&self, artifact: &Artifact, constraints: Option<&Value>) -> ScanVerdict;
}
```

**Non-goals:** ArtifactGuard is not a sandbox. It is a static pre-flight check. Runtime isolation is the Executor's responsibility (subprocess with restricted env, cgroups, etc.). Defense in depth means both layers exist.

---

## 9. Session Lifecycle

A **Session** is the lifetime of the running Moss process. It holds at most one active Blackboard at any time, plus references to Crystals produced from previously sealed Blackboards. A single session typically has few Blackboards — the active one stays open across follow-ups and is only sealed when the topic changes.

```
[Moss starts]
      |
      v
  Create Session (new Uuid)
      |
      v
  Wait for user input ◄──────────────────────────────────────┐
      |                                                       │
      v                                                       │
  Is there an active Blackboard?                              │
      |                                                       │
   NO |          YES                                          │
      |           |                                           │
      v           v                                           │
  Create new    Orchestrator.decompose()                      │
  Blackboard    with full board state                         │
      |           |                                           │
      |      +---------+                                      │
      |      |         |                                      │
      |  Follow-up  New topic                                 │
      |      |         |                                      │
      |      |     Seal current board → Crystal → M3          │
      |      |     Create new Blackboard                      │
      |      |         |                                      │
      +------+---------+                                      │
      |                                                       │
      v                                                       │
  Update intent, insert new Gaps                              │
  Runner.execute() (Active state)                             │
      |                                                       │
    +-+-----------------------------------+                   │
    |                                     |                   │
    v                                     v                   │
 All Gaps Closed                   Gated Gaps remain          │
    |                              (user approval needed)     │
    |                                     |                   │
    |                         Surface Gates; await input      │
    |                              approve / reject           │
    |                                     |                   │
    |                         Gap → Ready / Closed            │
    |                                     |                   │
    +<------------------------------------+                   │
    |                                                         │
    v                                                         │
  Orchestrator.synthesize() → response (Idle state)           │
  Return response to user                                     │
      |                                                       │
      └───────────────────────────────────────────────────────┘

[Session ends only on user exit or process crash → seal active board]
```

**Key invariants:**
- At most one Blackboard is active (Created/Active/Idle) per session at any time.
- A Blackboard stays open across follow-up messages. It is sealed only on topic change or session end.
- Gated interactions happen within the Blackboard's Active state — the board is never sealed while Gates are pending.
- A Sealed Blackboard is an immutable historical record, compressed into a Crystal in M3.
- The session has no idle timeout. It lives until the user exits or the process crashes.

**Crystallization** happens when a Blackboard is sealed: the board's outcomes are compressed into a Knowledge Crystal saved to M3. Only Blackboards with at least one Closed Gap with `EvidenceStatus::Success` produce a Crystal.

---

## 10. Blackboard Lifecycle

A Blackboard is a **workspace**, not a transaction. It stays open across multiple user messages as long as the conversation remains related. The Orchestrator appends new Gaps on each follow-up, and the intent evolves to capture the growing scope. A new Blackboard is created only when the Orchestrator determines the user has moved to an unrelated topic, or when the session ends.

### 10.1 Lifecycle States

```
Created ──> Active ──> Idle ──> Active  (follow-up adds new Gaps)
                         │
                         └──> Sealed    (new topic, or session ends)
```

| State | Description |
|---|---|
| **Created** | `Orchestrator::run` instantiates a new `Blackboard` (fresh `Uuid`). Intent is not yet set; Gap DAG is empty. |
| **Active** | Gaps are in flight. `drive_gaps` is executing. The Blackboard accepts writes: Gap state changes, Evidence appends, approval registrations. |
| **Idle** | All current Gaps have reached a terminal state (`Closed`). Synthesis has returned a response to the user. **The Blackboard remains writable** — new Gaps can be inserted on the next user message. It is waiting for input. |
| **Sealed** | Crystallized and immutable. The Blackboard has been compressed into a Knowledge Crystal (M3) and is never written to again. |

### 10.2 Lifecycle Transitions

**Created → Active:** The first `orchestrator.decompose()` call sets the intent and inserts the initial Gaps. `drive_gaps` begins execution.

**Active → Idle:** `blackboard.all_closed()` returns `true`. `drive_gaps` exits. Synthesis runs and the response is returned to the user. The Blackboard stays in memory, holding all Gaps and Evidence, waiting for the next message.

**Active → Active (HITL):** When one or more gaps are Gated, those gap tasks are still alive on the `drive_gaps` JoinSet, awaiting human I/O. The Blackboard stays Active. The CLI receives `ApprovalRequested` events via broadcast and surfaces them inline. The user enters `y` or `N` in the `[y/N]` prompt, which sends on the gap's `oneshot` channel. The gap task resumes (or closes), the loop picks up the completion, promotes dependents, and continues. No special HITL loop — this is just normal async I/O within the execution loop.

**Idle → Active (follow-up):** A new user message arrives. `Orchestrator::run` calls `decompose()` with the new query and the full Blackboard state. The `Decomposition.is_follow_up` flag is `true`. The Orchestrator updates the intent, inserts the new Gaps, and calls `drive_gaps` again. New Gaps may declare dependencies on existing Closed Gaps — those dependencies are already satisfied, so the new Gaps promote to Ready immediately.

**Idle → Sealed (new topic):** A new user message arrives, but `Decomposition.is_follow_up` is `false`. `Orchestrator::run` creates a fresh Blackboard and runs the decompose output against it. (Sealing + crystallization is a TODO — currently only the fresh board is created.).

**Idle → Sealed (session end):** The user exits or the process crashes. The Blackboard is crystallized. (Sealing on exit is a TODO.)

### 10.3 Intent Evolution

The Blackboard's `intent` is **mutable**. Each decompose call may refine it to reflect the user's evolving goal.

```
Round 1: "Book me a flight to Tokyo"
  → intent: "Book a flight to Tokyo"

Round 2: "Make it business class"
  → intent: "Book a business class flight to Tokyo"

Round 3: "Also find a hotel near Shibuya for 3 nights"
  → intent: "Book a business class flight to Tokyo and a hotel near Shibuya for 3 nights"
```

The intent is a living summary of what the user is trying to accomplish on this Blackboard. The Orchestrator updates it on every decompose call. The synthesis step reads the current intent (not the original) to produce the final response.

### 10.4 Growable Gap DAG

Gaps are append-only. Once inserted, a Gap is never removed from the Blackboard. New Gaps are added on each follow-up, and they can reference any existing Gap by name in their `dependencies` field.

```
Round 1 inserts:
  search_flights (Closed) ──> select_best_flight (Closed)

Round 2 inserts:
  upgrade_to_business (depends on select_best_flight — already Closed, so immediately Ready)

Round 3 inserts:
  search_hotels (no deps — immediately Ready)
  book_hotel (depends on search_hotels)
```

The Gap DAG grows monotonically. Closed Gaps from prior rounds are inert — the Runner skips them. `promote_unblocked()` and `drain_ready()` naturally handle the mix of old Closed Gaps and new Ready/Blocked Gaps without any changes to the scheduling logic.

**Name uniqueness:** Gap names must be unique across the entire Blackboard lifetime. The `name_index` enforces this. The decompose prompt instructs the LLM not to reuse names already on the board.

### 10.5 New-Topic Detection

There is no separate classifier. The Orchestrator's `decompose` call returns `is_follow_up: bool` — the LLM decides based on the full Blackboard state in the prompt. `Orchestrator::run` reads this flag directly to decide whether to reuse the current board or create a fresh one.

### 10.6 Ownership and Creation

`Orchestrator` is the sole owner of the Blackboard lifecycle within a session. The Compiler and Executor receive `Arc<Blackboard>` and may read/write Gaps and Evidence, but only the Orchestrator creates Blackboards.

```rust
// As implemented — Orchestrator::run
pub(crate) async fn run(&self, query: &str) -> Result<String, MossError> {
    let board = self.blackboard.lock().unwrap().clone();

    // Decompose — LLM sees full board snapshot, returns intent + is_follow_up + gaps
    let decomposition = self.decompose(query, &board).await?;

    // Reuse current board or create fresh one based on is_follow_up
    let board = if decomposition.is_follow_up {
        board
    } else {
        // TODO: seal old board → Crystal → M3
        let fresh = Arc::new(Blackboard::new(self.tx.clone()));
        *self.blackboard.lock().unwrap() = Arc::clone(&fresh);
        fresh
    };

    if let Some(ref intent) = decomposition.intent {
        board.set_intent(intent.as_str());
    }
    for spec in decomposition.gaps.unwrap_or_default() {
        board.insert_gap(Gap::new(spec.name, spec.description, spec.gap_type, ...))?;
    }

    // JoinSet fan-out: Compiler → ArtifactGuard → (HITL?) → Executor per gap
    self.drive_gaps(Arc::clone(&board)).await?;
    self.synthesize(&board).await
}
```

### 10.7 Invariants

- A Blackboard's Gap array only grows. Gaps are never removed or replaced.
- The intent is mutable — updated by the Orchestrator on each decompose call. The synthesis step always reads the latest intent.
- A Sealed Blackboard is immutable. No code path writes to it after crystallization.
- There is at most one active (Created/Active/Idle) Blackboard per session at any time.
- Sealing happens in two cases only: the Orchestrator signals a new topic, or the session ends.

---

## 11. Gap Lifecycle

```
Blocked ──> Ready ──> Assigned ──> Gated ──> Ready  (on user approval)
                                 │
                                 └─────────> Closed  (on user rejection)
                    Assigned ──────────────> Closed  (normal completion)
```

This is a one-directional state machine. The only backward arc is `Gated → Ready`, which requires explicit user action.

| State | Entry condition | Exit condition |
|---|---|---|
| **Blocked** | Gap has dependencies that are not yet Closed | All dependencies reach Closed state; auto-promoted to Ready by `promote_unblocked()` |
| **Ready** | No unresolved dependencies; eligible for scheduling | Picked up by `drive_gaps` and marked Assigned |
| **Assigned** | Compiler has been invoked; Executor is running | Executor posts Evidence and marks the gap Closed, OR ArtifactGuard gates the gap → Gated |
| **Gated** | Gap needs human action (security approval, user input, judgment call, physical action). The gap task stays alive on the JoinSet awaiting a `oneshot` response — this is just async I/O. `drive_gaps` has no gate-specific logic. | User enters `y` at `[y/N]` prompt → task resumes execution; user enters `N` → Closed with terminal failure |
| **Closed** | Terminal. The gap is resolved (success, terminal failure, or user rejection) | — |

**Gaps with no dependencies** skip Blocked and are inserted directly as Ready.

**Terminal failure:** A gap can be Closed with `Evidence.status = EvidenceStatus::Failure { reason }`. Downstream gaps that depend on a terminally-failed gap are also marked as terminally failed without execution — the Orchestrator propagates failure through the DAG.

---

## 12. Error Handling Strategy `PLANNED`

The current codebase uses `.expect()` and `panic!()` pervasively. For a daemon process, panics are fatal. The error handling strategy going forward:

**Crate-level error type:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum MossError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("compiler error for gap {gap_id}: {reason}")]
    Compiler { gap_id: Uuid, reason: String },

    #[error("executor error for gap {gap_id}: {reason}")]
    Executor { gap_id: Uuid, reason: String },

    #[error("defense scan rejected artifact: {reason}")]
    DefenseRejection { reason: String },

    #[error("blackboard error: {0}")]
    Blackboard(String),

    #[error("deadlock: blocked gaps remain but no gaps are ready or assigned")]
    Deadlock,

    #[error("MCP tool error: {tool} — {reason}")]
    Mcp { tool: String, reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

**Policy:** Every function that can fail returns `Result<T, MossError>`. The top-level `main.rs` loop catches errors and prints them to stderr without crashing. Individual Gap failures are isolated — they do not bring down the session.

---

## 13. Architecture Decisions

### ADR-001: Blackboard Pattern over Message-Passing Agents

**Status:** Accepted

**Context:** The system needs to coordinate multiple specialist tasks (web search, file operations, code generation) that operate on shared context. Two common patterns: (a) Blackboard — shared memory with a central coordinator reading/writing, (b) Actor/message-passing — each agent has private state and communicates via async channels.

**Decision:** Blackboard pattern, implemented with `DashMap` for concurrent access.

**Rationale:** The Orchestrator needs a global view of all Gaps and Evidence to make scheduling decisions and detect deadlocks. With message-passing, this global view requires either a centralized broker (which is functionally a Blackboard) or expensive all-to-all communication. The Blackboard makes the shared state explicit and inspectable, which simplifies debugging and enables the HUD to stream deltas directly from the data structure.

**Trade-offs:** Blackboard contention under very high parallelism (mitigated by DashMap's per-shard locking). Less isolation between components than pure message-passing. The DashMap approach means we cannot trivially distribute across processes — this is acceptable for a single-machine AIOS.

### ADR-002: Rust as Implementation Language

**Status:** Accepted

**Context:** The system is a local daemon with hard latency requirements (sub-second response to scheduling decisions) and concurrent execution of LLM calls, subprocesses, and tool invocations.

**Decision:** Rust with Tokio async runtime.

**Rationale:** Zero-cost async, no GC pauses, strong type system for modeling state machines (Gap lifecycle), and excellent subprocess management. The `DashMap` + `tokio::JoinSet` combination gives us concurrent DAG execution without manual thread management.

**Trade-offs:** Slower iteration speed than Python. Smaller ecosystem for LLM tooling (though `async-openai` and the MCP Rust SDK exist). Higher learning curve for contributors.

### ADR-003: LLM-Generated Code as the Execution Primitive

**Status:** Accepted

**Context:** Gaps need to be resolved by "doing something" — calling APIs, transforming data, navigating websites. Options: (a) a fixed toolkit of Rust-native functions the LLM selects from, (b) LLM generates executable code (scripts) on the fly.

**Decision:** LLM generates code. The Compiler produces Python scripts or agent specs.

**Rationale:** A fixed toolkit scales linearly with development effort and suffers from selection errors as it grows (the LLM must choose from an ever-larger menu). Code generation scales with the LLM's capability: as models improve, the range of solvable Gaps expands without code changes to Moss. Scripts are also inspectable and auditable (logged to M2).

**Trade-offs:** Security risk from executing LLM-generated code (mitigated by DefenseClaw + sandboxing). Latency overhead of an extra LLM call per Gap (mitigated by parallelism). Debugging is harder when the execution logic is generated at runtime.

### ADR-004: Micro-Agent = ReAct Loop, Not Recursive Orchestrator

**Status:** Accepted

**Context:** Reactive Gaps require non-deterministic real-world interaction (web browsing, API discovery, multi-step tool use). The initial design proposed spawning a recursive Orchestrator + Blackboard pair for each Reactive Gap.

**Decision:** A Reactive Gap is executed by a `MicroAgent` running a ReAct (Reason → Act → Observe) loop. It does not instantiate a new Orchestrator, does not have a sub-Blackboard, and does not call the Compiler. The only output is a single `Evidence` record posted to the parent Blackboard.

**Rationale:** The Blackboard pattern exists to coordinate parallel planning across multiple independent tasks. A ReAct loop is inherently sequential and self-contained. Giving it a full Orchestrator adds two extra LLM calls (decompose + synthesize), a sub-Blackboard that has no observability from the parent, and unbounded recursion risk. The MicroAgent struct is simpler, faster to implement, and its entire execution is scoped to one Gap.

**Trade-offs:** A MicroAgent cannot itself spawn parallel sub-tasks. If a Reactive Gap is genuinely complex enough to warrant parallel decomposition, it should be decomposed at planning time by the Orchestrator into multiple Gaps — not at runtime inside a MicroAgent.

### ADR-005: Human-in-the-Loop via `GapState::Gated`

**Status:** Accepted — updated by ADR-007 (HITL gating as I/O).

**Context:** Some Gap actions require human involvement before they can proceed. This includes security-sensitive actions (deleting files, sending email, making purchases) flagged by DefenseClaw, but also any situation where the system needs human input, judgment, or physical action (entering a 2FA code, choosing between options, confirming a preference).

**Decision:** When any component determines a Gap needs human action, the Gap transitions to `Gated` state and a Gate is inserted into the Blackboard (which emits `Signal::GateRequested` via the `SignalBus` — see ADR-008). The gap task **stays alive on the JoinSet**, awaiting the human response on a per-gate `oneshot` channel — this is just I/O, no different from awaiting a web request. The Runner has no gate-specific logic; it processes one JoinSet completion per iteration and loops. Other gaps keep executing concurrently while the human acts. The CLI subscribes to the `SignalBus` and surfaces Gate prompts in real-time. The user runs `approve <name>` or `reject <name>`, which sends on the gate's `oneshot`. On approval, the gap task resumes execution. On rejection, the Gap posts Failure evidence and transitions to Closed.

**Rationale:** `Gated` is a first-class state for **observability** (HUD, planner view, CLI display). The Runner doesn't check for it — it just sees async tasks on the JoinSet. Human latency doesn't block unrelated gaps because the JoinSet processes completions incrementally (one at a time), not in batch. The terminal condition is simply `all_closed()` — `all_gated_or_closed()` is removed.

**Trade-offs:** A Gated Gap blocks all downstream Gaps that depend on it, since they cannot promote from Blocked until their dependency is Closed. This is correct — downstream tasks that depend on a human-gated action cannot proceed until that action is confirmed. Independent branches are unaffected.

### ADR-006: Living Blackboard with Mutable Intent and Growable DAG

**Status:** Accepted — supersedes the "round-scoped immutable Blackboard" design from v0.4.

**Context:** The original design created a fresh Blackboard for every user message and sealed it immediately after synthesis. Follow-ups required reconstructing context from M1 summaries or M3 Crystals — a lossy process that threw away the rich Evidence the system just produced. The sealed-per-round model also introduced an artificial lifecycle boundary that didn't match how users actually interact: a follow-up like "make it business class" after "book a flight" is clearly the same conversation thread, not a new one.

**Decision:** A Blackboard is a living workspace. It stays open across follow-up messages. On each user input, the Orchestrator receives the full Blackboard state (intent + Gaps + Evidence summaries), returns an updated intent and new Gaps. New Gaps are appended — the Gap array only grows. The intent is mutable and evolves to reflect the user's expanding scope. The Blackboard is sealed only when the Orchestrator's decompose output signals a new, unrelated topic, or when the session ends.

**Rationale:**
- Follow-ups get full-fidelity access to prior Evidence — no information loss from summarization or crystallization.
- The Runner and DAG scheduler require zero changes: `drain_ready()` skips Closed Gaps, `promote_unblocked()` handles dependencies on already-Closed Gaps naturally, `insert_gap()` works on a board with existing Closed Gaps.
- New-topic detection is absorbed into the decompose call — no separate classifier, no extra LLM call, no explicit mode flag. `Decomposition::is_follow_up` carries the decision.
- The Orchestrator already receives the Blackboard state for planning. Asking it to also refine the intent and decide topic continuity adds zero cost.

**Trade-offs:**
- A long-running Blackboard accumulates many Gaps and Evidence records. The `snapshot()` serialization sent to the Orchestrator could grow large. Mitigation: summarize Evidence in the planner view rather than including raw content; cap the number of Gap entries shown to the LLM.
- Crystallization timing changes: Crystals are now produced less frequently (on topic change rather than every message). Each Crystal covers more ground, which may be better or worse for M3 retrieval precision. This is an open question to evaluate once M3 is implemented.
- The "new-topic" inference heuristic (no new Gaps reference existing ones + intent diverges) may have edge cases. If it proves unreliable, a fallback is an explicit user command (`/new`) to force a board seal.

---

## 14. Open Questions

These are unresolved design decisions that need answers before or during implementation.

1. ~~**Re-planning.**~~ **Closed — Decision:** No re-planning in v1. Terminal failure propagates through the DAG downstream. Dependent Gaps are marked Closed with `EvidenceStatus::Failure`. Re-planning is deferred to v2 and requires explicit plan versioning and a `replace_subgraph` API on the Blackboard.

2. **Streaming vs. batch Evidence.** Should the Executor post Evidence incrementally as a script produces output (streaming), or only after the script completes (batch)? Streaming enables the HUD to show progress, but complicates the "done" semantics on Evidence and the dependency resolution logic.

3. **Embedding model for M3.** Which embedding model for Knowledge Crystal vectors? Options: a local model (e.g., `nomic-embed-text` on the RTX 4090), or a remote API (e.g., OpenAI embeddings via OpenRouter). Local keeps it offline; remote is simpler to start with.

4. **Multi-user / multi-session.** The current design is single-user, single-session. If Moss ever serves multiple concurrent sessions (e.g., as a daemon handling multiple terminal windows), the Blackboard needs session-scoped namespacing and the Memory tiers need per-user isolation.

5. ~~**Micro-Agent recursion depth.**~~ **Closed — Decision:** Micro-Agents are flat ReAct loops. They do not instantiate a new Orchestrator, do not have a sub-Blackboard, and do not call the Compiler. Recursion depth is 0. Not applicable.

---

## 15. Implementation Status Matrix

| Component | Layer | Status | Notes |
|---|---|---|---|
| CLI input loop | L5 | `IMPLEMENTED` | `src/cli.rs` — `Cli` struct, async stdin reader, calls `Moss::run`, prints response |
| CLI signal handling | L5 | `IMPLEMENTED` | `src/cli.rs` — `tokio::select!` + signal receiver, surfaces `ApprovalRequested` inline, `[y/N]` prompt, calls `Moss::approve()` |
| HUD delta streamer | L5 | `PLANNED` | Requires Blackboard change notifications |
| Orchestrator decompose | L4 | `IMPLEMENTED` | `orchestrator.rs` — minijinja template, LLM call, JSON deserialization; `Decomposition` includes `is_follow_up` flag |
| Orchestrator synthesize | L4 | `IMPLEMENTED` | Full Evidence from Blackboard via `all_evidence()` |
| Orchestrator execution loop | L4 | `IMPLEMENTED` | `drive_gaps()` private method; `runner.rs` dissolved |
| Blackboard data structures | L3 | `IMPLEMENTED` | All types, private fields, `pub(crate)` getters |
| Blackboard insert/mutate | L3 | `IMPLEMENTED` | `insert_gap`, `set_gap_state`, `append_evidence`, `set_intent`, `register_approval`, `approve` |
| Blackboard dependency resolution | L3 | `IMPLEMENTED` | `promote_unblocked`, `drain_ready`, `all_closed` — unit tested. `all_gated_or_closed` removed (ADR-007). |
| Signal Bus integration | L3 | `IMPLEMENTED` | Every mutation emits `Event::Snapshot`. `register_approval`/`approve` for HITL oneshot. `signal_tx()` for external emission. |
| Compiler | L2 | `IMPLEMENTED` | `compiler.rs` — `Artifact` (Script/Agent), prior_attempts error strings, tested with LocalMock |
| Executor (script) | L2 | `IMPLEMENTED` | `executor.rs` — subprocess runner, timeout, Evidence to Blackboard |
| Executor (Micro-Agent) | L2 | `PLANNED` | Stub in `executor.rs` — writes Failure evidence. Phase 8. |
| M1 session context | L1 | `PLANNED — design open` | Cross-board awareness; structure TBD |
| M2 Sled store | L1 | `PLANNED` | `sled` not in Cargo.toml |
| M3 Qdrant integration | L1 | `PLANNED` | `qdrant-client` not in Cargo.toml |
| Provider trait | L0 | `IMPLEMENTED` | Returns `Result<String, ProviderError>` |
| OpenRouter provider | L0 | `IMPLEMENTED` | No `.expect()` — all errors through `ProviderError` |
| Provider error handling | L0 | `IMPLEMENTED` | `thiserror` in Cargo.toml; `MossError` + `ProviderError` defined |
| Provider streaming | L0 | `PLANNED` | — |
| Provider tool calling | L0 | `PLANNED` | `complete_with_tools` stub returns `Err(ProviderError::NotSupported)` |
| MCP bridge | L0 | `PLANNED` | — |
| DefenseClaw | L0 | `IMPLEMENTED` | `artifact_guard.rs` — `ArtifactGuard` zero-field struct, 4-stage scan, `HITL_PATTERNS` const |

**Recommended implementation order** (each phase is independently testable):

1. ~~**Error handling foundation.**~~ ✅ Done — `thiserror`, `MossError`, `ProviderError`, all `.expect()` removed.
2. ~~**Blackboard.**~~ ✅ Done — All data structures, `drain_ready`, `promote_unblocked`, `all_closed`, unit tested.
3. ~~**Orchestrator decompose + synthesize.**~~ ✅ Done — `minijinja` templates, LLM call, JSON parse, gap insertion in `Moss::run`.
4. ~~**Compiler.**~~ ✅ Done — `prompts/compiler.md`, Provider call, `Artifact` (Script/Agent), tested with LocalMock.
5. ~~**Executor (script path).**~~ ✅ Done — subprocess runner with timeout, Evidence written to Blackboard.
6. ~~**Signal Bus + Runner rewrite + CLI async loop (ADR-008).**~~ ✅ Done — `signal.rs` broadcast, `drive_gaps` on Orchestrator, `Cli` with `tokio::select!`.
7. ~~**DefenseClaw.**~~ ✅ Done — `ArtifactGuard`: 4-stage scan, HITL oneshot round-trip, `ApprovalRequested` signal.
8. **MCP bridge + Agent Loop.** Connect to at least one MCP server (filesystem). Implement `MicroAgent` ReAct loop for Reactive Gaps. MicroAgent can use `register_approval()` for human input mid-loop.
9. **Memory (M1).** Session context layer. Enables cross-board awareness after topic changes.
10. **Memory (M2/M3).** Sled + Qdrant. Enables cross-session learning.
11. **HUD.** Subscribes to `SignalBus` from phase 6. Renders `Signal` events as terminal deltas. No new infrastructure — just another consumer.
