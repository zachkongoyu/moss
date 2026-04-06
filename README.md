# Moss — AI Operating System

Moss is a local-first AI Operating System built in Rust. It transforms a single user intent into a parallel execution plan — a DAG of atomic tasks called Gaps — runs each one by generating and executing code, and synthesizes a final result.

The architecture follows the Blackboard pattern (Hearsay-II lineage): independent specialist components read from and write to shared, structured session memory, coordinated by a central Orchestrator.

**Full architecture specification:** [ARCHITECTURE.md](./ARCHITECTURE.md)

## Core Ideas

- **Living Blackboard.** A Blackboard stays open across follow-up messages. The Orchestrator appends new Gaps and refines the intent as the conversation evolves. A new Blackboard is created only when the topic changes or the session ends.
- **Code as the universal solver.** Every Gap is resolved by generating and running code — a deterministic script or a reactive agent loop — not by prompting the LLM to "think harder."
- **Failure containment.** A failing task cannot corrupt the global state. Reactive tasks run inside encapsulated Micro-Agent instances with an isolated ReAct loop.
- **Concurrency by default.** Independent Gaps execute in parallel via `tokio::JoinSet`. The DAG structure determines ordering.

## Quick Start

### Prerequisites

- Rust (2024 edition)
- An OpenRouter API key (or any OpenAI-compatible endpoint)

### Setup

```bash
git clone <repo-url> && cd moss

# Configure your LLM provider
cp .env.example .env
# Edit .env and set OPENROUTER_API_KEY

cargo build
cargo run
```

The CLI starts an interactive loop. Type a message and press Enter. Type `exit` to quit.

Set `RUST_LOG=moss=debug` (or `info` / `trace`) for pipeline logging.

### Project Structure

```
src/
  main.rs                       Entry point, CLI loop
  lib.rs                        Moss facade — public entry point
  error.rs                      MossError + ProviderError
  moss/
    blackboard.rs               Living workspace: Gaps, Evidence, Gates, intent
    orchestrator.rs             Decompose (intent → Gap DAG) + synthesize
    compiler.rs                 Gap → Artifact (Script or Agent)
    executor.rs                 Runs Artifacts, writes Evidence to Blackboard
    runner.rs                   JoinSet execution loop, retry, deadlock detection
    decomposition.rs            Decomposition DTO (LLM output)
    prompts/
      decompose.md              Planning prompt template
      compiler.md               Code generation prompt template
      synthesize.md             Synthesis prompt template
  providers/
    mod.rs                      Provider trait definition
    remote/
      openrouter.rs             OpenRouter API integration
    local/
      mod.rs                    Mock provider for testing
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (2024 edition) |
| Async runtime | Tokio |
| Concurrent state | DashMap |
| LLM access | OpenRouter (any OpenAI-compatible API) |
| Serialization | serde + serde_json |
| Template engine | minijinja |
| Tool protocol | MCP (Model Context Protocol) — planned |
| Vector store | Qdrant — planned |
| Local KV store | Sled — planned |

## Roadmap

See the **Implementation Status Matrix** in [ARCHITECTURE.md](./ARCHITECTURE.md#14-implementation-status-matrix) for detailed component status and recommended build order.

## Test Scenarios

These are the target capabilities, ordered by complexity:

1. **Basic Reflex.** Move a file from Downloads to a target directory using semantic search — no manual paths.
2. **Contextual Intuition.** Summarize PDF receipts from email, update a local expense spreadsheet.
3. **Advanced Chore.** Book the cheapest Tokyo flight for Friday on a previously used airline.
4. **Sovereign Intelligence.** Fix auth bugs in a Rust project, verify via web, and notify Slack.

## License

TBD
