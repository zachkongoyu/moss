# Phase 8 — Agent Loop (Reactive Gaps)

**Status:** Ready (no dependency on Phase 6)
**ADRs:** ADR-004 (MicroAgent = ReAct loop)
**Effort:** Medium — MCP bridge + MicroAgent + Provider tool-calling.

---

## Spec

Fills in the Executor's Agent stub.

**MicroAgent:**
- `MicroAgent { provider: Arc<dyn Provider>, tools: Vec<Box<str>>, max_iterations: u32 }`
- Runs a ReAct loop: Reason (LLM call) → Act (tool call) → Observe (result) → Reflect.
- Can gate via `insert_gate()` when it needs human input mid-loop.
- Output: single `Evidence` record posted to parent Blackboard.

**Provider tool-calling:**
- Implement `complete_with_tools` on `OpenRouter` (currently returns `Err(ProviderError::NotSupported)`).

**MCP bridge:**
- `src/providers/mcp.rs`: `discover()`, `call()`, `tool_definitions()`.
- Start with stdio transport to filesystem MCP server.

**Files:** `src/providers/mcp.rs` (new), `src/moss/micro_agent.rs` (new), `src/moss/executor.rs`, `src/providers/remote/openrouter.rs`
