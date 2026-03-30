# Moss AI Operating System (AIOS)

Moss is a bio-inspired, high-performance AI Operating System built from first principles in Rust.
It transforms personal computing into a proactive, collaborative intelligence partner by moving logic closer to the hardware and treating sessions as living, "fresh-start" cycles.

## 🧬 Core Philosophy

- **The Fresh Start**: Every session clears the Blackboard (L3) to ensure reasoning is never cluttered by irrelevant history.
- **Logits-based Snapshotting**: Saves mathematical "thought states" into a Thin Active Cache (L1), allowing for sub-10ms resume latency via load_context syscalls.
- **Knowledge Crystallization**: Post-mission, the Meta-Agent compresses outcomes into Durable Artifacts stored in a Vector DB (L2).
- **Bio-Inspired Performance**: Achieves a 2.1x increase in execution speed by bypassing the von Neumann bottleneck through unified high-density inference on RTX 4090 Tensor Cores.
- **Session-aware Evidence Injection**: The Synthesizer can inject relevant session context into the Blackboard's evidence stream to preserve continuity during a session. Sessions persist while active and are subject to an idle timeout (see Session lifecycle below).

- **Relevance Gate (Yes/No)**: Before injection, the Synthesizer can optionally run a quick relevance check (yes/no). If the session context is not relevant to the new query, the gate returns `No` and the Blackboard remains unaffected.

## 🗺️ Development Roadmap: The Genesis Loop

| Phase | Milestone | Focus |
| --- | --- | --- |
| Day 1 | The Seed | Core Rust Daemon, L5 CLI, and structured JSON Blackboard loop. |
| Day 2 | The Parallel Workforce | Thread-bound Expert Agents (Pulses) & Round Robin (RR) Scheduler. |
| Day 3 | The Sensory Bridge | MCP Integration for standardized AIOS Syscalls (Browsers/Files). |
| Day 4 | The Living Memory | Logits-based Context Manager for instant reasoning restoration. |
| Day 5 | The Final Synthesis | Knowledge Crystallization pipeline & Born Observable HUD telemetry. |

## 🧩 Architecture Diagram (Mermaid)

```mermaid
graph TD
    %% Layout groups
    subgraph L5_Interface [L5: Interface]
        direction LR
        User((User))
        CLI[Rust Daemon: CLI / HUD]
        HUD[HUD: Blackboard Delta Streamer]
    end

    subgraph L4_Orchestration [L4: Brain - Synthesizer]
        direction TB
        Brain[Synthesizer: Reasoning Core]
    end

    subgraph L3_Scratchpad [L3: Context - Blackboard]
        direction TB
        subgraph DashMaps [DashMap Memory - Per Query]
            Intent[intent]
            Gaps[gaps: Work DAG]
            Evidence[evidence: Structured Data]
            Gates[gates: HITL Callbacks]
            Relevance{"Use session context? (Yes / No)"}
        end
    end

    subgraph L2_Workforce [L2: Hands - Stateless Pulses]
        direction TB
        JoinSet{tokio::JoinSet}
        NPulse[Network Pulse]
        MPulse[Machine Pulse]
    end

    subgraph L1_Memory_Tier [L1: Living Memory Hierarchy]
        direction TB
        M1["Session Context: Ring Buffer (30m idle timeout)"]
        M2[Sled DB: Local Prefs & Audit]
        M3[Qdrant: Knowledge Crystals]
    end

    subgraph L0_Infra [L0-L2.5: Infrastructure & Execution]
        direction TB
        LLM[LLM Core: Local RTX 4090 / Remote API]
        MCP[MCP Bridge: Tool / App Integration]
        Defense[DefenseClaw: Pre-Execution Scanner]
    end

    %% --- CONNECTIONS ---
    CLI -->|Intent| Brain
    M3 -.->|Semantic Retrieval| Brain
    M1 -.->|load_context| Brain

    Brain -->|Initialize DAG| Gaps
    Brain -->|Check relevance| Relevance
    Relevance -->|Yes| Evidence
    Relevance -->|No| Gaps

    Gaps -->|Poll Ready Tasks| JoinSet
    JoinSet --> NPulse & MPulse
    NPulse & MPulse -.->|Update Progress/Deps| Gaps

    NPulse -->|Request Tool Use| MCP
    MPulse -->|Code Execution| Defense
    NPulse & MPulse -->|Logical Inference| LLM

    LLM & MCP & Defense -->|Post Result| Evidence
    Evidence -.->|Resolve Dependencies| Gaps

    Gaps --"Stream"--> HUD
    Evidence --"Stream"--> HUD

    Gaps -->|Terminal Node Met| Brain
    M1 -.->|idle > 30m: expire session| Brain
    Brain -->|Response Synthesis| CLI
    DashMaps -->|Crystallization| M3

    %% Styling
    classDef interface fill:#EEF2FF,stroke:#2563EB,stroke-width:2px,color:#0f172a;
    classDef brain fill:#FEF3C7,stroke:#D97706,stroke-width:2px,color:#0f172a;
    classDef scratch fill:#ECFCCB,stroke:#16A34A,stroke-width:2px,color:#064E3B;
    classDef workforce fill:#F0F9FF,stroke:#0EA5E9,stroke-width:2px,color:#0B3A4B;
    classDef memory fill:#FCE7F3,stroke:#DB2777,stroke-width:2px,color:#4C0033;
    classDef infra fill:#F8FAF9,stroke:#64748B,stroke-width:2px,color:#0F172A;
    classDef gate fill:#FFF7ED,stroke:#EA580C,stroke-width:2px,color:#92400E;

    class User,CLI,HUD interface;
    class Brain brain;
    class Intent,Gaps,Evidence,Gates scratch;
    class JoinSet,NPulse,MPulse workforce;
    class M1,M2,M3 memory;
    class LLM,MCP,Defense infra;
    class Relevance gate;

    linkStyle default stroke:#94A3B8,stroke-width:1.5px;
    linkStyle 0 stroke:#60A5FA,stroke-width:2px;
    %% Session context injection
    Brain -->|Inject Session Context into Evidence| Evidence
```

### Session lifecycle

- **Duration & expiry**: Sessions are kept live to preserve context up to a 30-minute idle timeout. If a session is idle for more than 30 minutes it is cleared from the Blackboard (L3) and subsequent interactions start a fresh session.

## 🧪 Baseline Test Scenarios

### 🕹️ Level 1: Basic Reflex
- Scenario: Move a high-res photo from Downloads to primary memory.
- Metric: Machine Pulse executes semantic search and move syscall without manual paths.

### 🧠 Level 2: Contextual Intuition
- Scenario: Summarize PDF receipts from email and update local expense spreadsheet.
- Metric: Network Pulse retrieves data via MCP; Machine Pulse performs local writes.

### 🌐 Level 3: Advanced Chore
- Scenario: Book the cheapest Tokyo flight for Friday on a previously used airline.
- Metric: Semantic retrieval of preferences + dynamic web orchestration.

### 🛡️ Level 4: Sovereign Intelligence
- Scenario: Fix auth bugs in a Rust project, verify via web, and notify Slack.
- Metric: Error interpretation + autonomous recovery + DefenseClaw safety scanning.

## ⚙️ High-Performance Tech Stack (2026)

- Reasoning Core: DeepSeek-V3.2-Exp / GLM-4.5-Air (Optimized for tool/web use).
- Governance: DefenseClaw (Pre-execution runtime scanning).
- Protocol: MCP (Model Context Protocol).
- Hardware: NVIDIA RTX 4090 (vLLM core, unified safety + reasoning pipeline).
