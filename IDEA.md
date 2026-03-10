
# Roughneck: Deep-Agent Harness for Rig (High-Level Design Spec)

## 1. Purpose & Scope

**Roughneck** is a Rust-based deep-agent harness built on top of the **Rig** ecosystem for building LLM-powered applications. It implements the “deep agent” pattern from LangChain DeepAgents—planning, sub-agents, filesystem tools, skills, long-term memory, and MCP integration—but in a **Rust-first, multi-crate** architecture with first-class bindings to:

- **Python** via **PyO3**
- **TypeScript/Node** via **napi-rs**

This spec is intended to be fed into an AI coding assistant (e.g., Claude, Codex, Cursor) to scaffold the initial implementation.

### Goals

- Provide a **reusable deep-agent harness** similar to LangChain DeepAgents, but using **Rig** as the underlying LLM/agent stack.
- Support:
  - **Planning tools** (todo/plan)  
  - **Sub-agents / task delegation**  
  - **Skills** (declarative capabilities loaded into system prompt + tools)  
  - **MCP** tools as first-class integrations  
  - **Pluggable filesystem** and **tool** backends  
  - **Pluggable memory** abstraction (short-term and long-term)
- Provide ergonomic, high-level APIs for:
  - **Rust** (direct API)
  - **Python** (`deepagents`-style `create_deep_agent`)
  - **Node/TS** (`createDeepAgent` API mirroring JS DeepAgents)

### Non‑Goals (Initial Version)

- Full 1:1 feature parity with LangGraph’s runtime (e.g., full DAG graphs, advanced checkpointers).
- GUI/desktop runner (CLI only initially).
- Non-Rust host bindings (e.g., Ruby, Java) beyond Python and Node.

---

## 2. Workspace & Crate Layout

Use a **Cargo workspace**:

```text
roughneck/
  Cargo.toml              # [workspace]
  crates/
    roughneck-core/
    roughneck-runtime/
    roughneck-fs/
    roughneck-memory/
    roughneck-skills/
    roughneck-mcp/
    roughneck-cli/
    roughneck-py/         # PyO3 bindings
    roughneck-node/       # napi-rs bindings
```

### Crate Responsibilities

- **`roughneck-core`**  
  Core traits, data types and error types:
  - `Agent`, `DeepAgent`, `Tool`, `ToolCall`, `ToolResult`
  - `PlanningTool`, `SubAgent`, `Skill`, `MemoryBackend`, `FileSystemBackend`
  - Common config structs and enums.

- **`roughneck-runtime`**  
  Deep-agent harness implementation:
  - The main **planning + tool-calling loop** (deep-agent behavior).
  - Integration with **Rig** models and agents (providers, embeddings, vector stores).
  - Orchestration of planning, sub-agents, filesystem, memory, skills, MCP.

- **`roughneck-fs`**  
  Filesystem backends and filesystem tools:
  - Backends: in-memory, local FS, optional store-backed.
  - Tools: `ls`, `read_file`, `write_file`, `edit_file`, `glob`, `grep`, `execute` (naming aligned with LangChain DeepAgents and deepagents.js).

- **`roughneck-memory`**  
  Pluggable memory abstraction:
  - Short-term (conversation-window aware) and long-term (persistent) memory interfaces.
  - Implementations (e.g., in-memory, SQLite, Redis, file-backed).

- **`roughneck-skills`**  
  Skill system:
  - Load “skills” definitions (YAML/JSON/TOML/Markdown) and inject into system prompt.
  - Expose skill-specific tools and metadata (similar to DeepAgents skills middleware).

- **`roughneck-mcp`**  
  MCP integration:
  - Connect to MCP servers (per MCP spec) and expose MCP tools as Roughneck tools.
  - Handle tool discovery, schema mapping, and invocation.

- **`roughneck-cli`**  
  CLI harness:
  - Terminal deep-agent (similar to DeepAgents CLI for coding) with planning, fs, sub-agents, etc.

- **`roughneck-py`**  
  Python bindings via PyO3:
  - Expose Pythonic `create_deep_agent(...)` API modeled after LangChain DeepAgents.

- **`roughneck-node`**  
  Node/TS bindings via napi-rs:
  - Expose `createDeepAgent({ ... })` API modeled after deepagents.js.

---

## 3. Core Concepts & Types (`roughneck-core`)

### 3.1 Agent & DeepAgent

```rust
pub struct DeepAgentConfig {
    pub system_prompt: String,
    pub tools: Vec<Arc<dyn Tool>>,
    pub planning: PlanningConfig,
    pub subagents: Vec<SubAgentConfig>,
    pub filesystem: FileSystemConfig,
    pub memory: MemoryConfig,
    pub skills: SkillsConfig,
    pub mcp: McpConfig,
}

pub struct DeepAgent {
    pub id: String,
    pub config: DeepAgentConfig,
    pub client: Arc<dyn CompletionClient>,  // from rig
}
```

- `DeepAgent` wraps a **Rig** completion/agent client and orchestrates planning, tools, sub-agents, filesystem and memory.

### 3.2 Tools

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;          // e.g. "ls", "write_todos", "call_subagent"
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value; // JSON Schema
    async fn call(&self, args: Value, ctx: &ToolContext) -> ToolResult;
}

pub struct ToolContext {
    pub fs: Arc<dyn FileSystemBackend>,
    pub memory: Arc<dyn MemoryBackend>,
    pub subagents: Arc<SubAgentRegistry>,
    pub skills: Arc<SkillsRegistry>,
    pub mcp: Arc<McpClientRegistry>,
    pub metadata: HashMap<String, Value>,
}
```

### 3.3 Planning

- **Planning tool** should mimic DeepAgents’ `write_todos` behavior: record a todo list in state while being a no-op externally.

```rust
pub struct PlanningConfig {
    pub enabled: bool,
    pub tool_name: String,  // default: "write_todos"
}

pub struct WriteTodosTool { /* ... */ }
```

The tool:

- Name: `"write_todos"`  
- Input schema: list of tasks (`[{ "task": "string", "status": "pending|done" }, ...]`)  
- Writes plan into memory / agent state but returns a small textual confirmation.

### 3.4 Sub-Agents

```rust
pub struct SubAgentConfig {
    pub name: String;       // "research-agent"
    pub description: String;
    pub system_prompt: String;
    pub tools: Vec<Arc<dyn Tool>>;
    pub model: Option<String>; // override Rig model ID
}

pub struct SubAgentRegistry {
    inner: HashMap<String, SubAgentHandle>,
}
```

- Sub-agents are “child” DeepAgents with their own prompts/tools/models, invoked via a dedicated tool:

```rust
pub struct CallSubagentTool { /* name = "call_subagent" */ }
```

`call_subagent` input:

```json
{
  "subagent": "research-agent",
  "task": "Investigate XYZ",
  "context_files": ["notes/research.md"]
}
```

Return: textual summary + optional file writes (via fs).

### 3.5 Skills

- Skills are named capability bundles (prompt snippets + tools) that can be **loaded and enabled** for an agent.

```rust
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub system_instructions: String,
    pub tools: Vec<Arc<dyn Tool>>,
}

pub struct SkillsConfig {
    pub enabled_skills: Vec<String>;       // skill IDs
    pub registry_paths: Vec<PathBuf>;      // directories to scan
}
```

`roughneck-skills` will provide:

- `SkillsRegistry` that loads definitions from disk (YAML/TOML/JSON/Markdown).
- Middleware logic to assemble system prompt and tool list.

### 3.6 Memory

```rust
#[async_trait::async_trait]
pub trait MemoryBackend: Send + Sync {
    async fn append_event(&self, conv_id: &str, event: MemoryEvent) -> Result<()>;
    async fn get_events(&self, conv_id: &str, limit: usize) -> Result<Vec<MemoryEvent>>;
    async fn search(
        &self,
        conv_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEvent>>;
}

pub enum MemoryScope {
    ShortTerm,
    LongTerm,
}
```

Config:

```rust
pub struct MemoryConfig {
    pub backend: MemoryBackendKind; // e.g. InMemory, Sqlite, Custom(Box<dyn MemoryBackend>)
    pub short_term_limit: usize;    // messages kept in working window
}
```

---

## 4. Runtime Flow (`roughneck-runtime`)

### 4.1 High-Level Agent Loop

Pseudocode for `DeepAgent::invoke`:

```rust
pub struct InvokeRequest {
    pub messages: Vec<ChatMessage>,  // user + prior history
    pub files: HashMap<String, String>, // virtual FS snapshot
}

pub struct InvokeResponse {
    pub messages: Vec<ChatMessage>,
    pub files: HashMap<String, String>,
}

impl DeepAgent {
    pub async fn invoke(&self, req: InvokeRequest) -> Result<InvokeResponse> {
        // 1. Seed memory + filesystem state from request
        // 2. Build system prompt from:
        //    - base instructions
        //    - skills
        //    - filesystem/memory hints
        //    - planning/subagent instructions
        // 3. Enter tool-calling loop (similar to DeepAgents / tool-calling agents):
        //    - Send messages + tool outputs to Rig model (tool-calling mode)
        //    - If model returns tool calls:
        //        - Dispatch to appropriate Tool implementations
        //        - Update memory/fs/subagents/skills state
        //    - If model returns final answer, exit
        // 4. Return final messages + updated files snapshot.
    }
}
```

- Use **Rig** completion/agent abstractions for tool-calling and streaming where possible.

### 4.2 System Prompt Construction

System prompt composition should mirror DeepAgents:

- Core harness instructions (how to:
  - use `write_todos` planning tool  
  - use filesystem tools (`ls`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `execute`)  
  - spawn subagents via `call_subagent`  
  - leverage skills + MCP tools  
  - persist and retrieve memory
- Custom app-level `system_prompt` from `DeepAgentConfig`.
- Skill-specific instructions appended in a skills-aware section.

---

## 5. Filesystem & Tools (`roughneck-fs`)

### 5.1 FileSystemBackend Trait

```rust
#[async_trait::async_trait]
pub trait FileSystemBackend: Send + Sync {
    async fn ls(&self, path: &str) -> Result<Vec<FileInfo>>;
    async fn read_file(&self, path: &str, range: Option<LineRange>) -> Result<String>;
    async fn write_file(&self, path: &str, content: &str) -> Result<()>;
    async fn edit_file(&self, path: &str, patch: FilePatch) -> Result<()>;
    async fn glob(&self, pattern: &str) -> Result<Vec<FileInfo>>;
    async fn grep(&self, pattern: &str, paths: Vec<String>) -> Result<Vec<GrepMatch>>;
    async fn execute(&self, cmd: &str, timeout: Duration) -> Result<ExecutionResult>;
}
```

Backends:

- `InMemoryBackend` (ephemeral, used for tests/CLI).
- `LocalFsBackend` (backed by host filesystem).
- Optional `StoreFsBackend` that uses a memory store (for persistent, distributed use).

### 5.2 Tool Naming (Align with LangChain / Claude)

Expose the following tools (names match DeepAgents & Claude naming where possible):

- `"ls"` – list directory contents
- `"read_file"`
- `"write_file"`
- `"edit_file"`
- `"glob"`
- `"grep"`
- `"execute"` – run shell commands (only enabled when backed by sandbox/secured backend)
- `"write_todos"` – planning tool
- `"call_subagent"` – sub-agent delegation

Tool input/outputs should follow JSON-schema conventions so they can be expressed to Rig and to bindings easily.

---

## 6. MCP Integration (`roughneck-mcp`)

### 6.1 MCP Client Abstraction

```rust
pub struct McpServerConfig {
    pub name: String;
    pub endpoint: Url;
    pub token: Option<String>;
}

pub struct McpClient {
    // connection pooling, schema cache, etc.
}

pub struct McpClientRegistry {
    inner: HashMap<String, Arc<McpClient>>,
}
```

### 6.2 Tool Exposure

- At startup, fetch tool schemas from configured MCP servers.
- For each MCP tool, create a dynamic `Tool` implementation that:
  - Uses MCP schemas for input validation.
  - For invocation, sends tool call over MCP protocol and maps result back to `ToolResult`.

Optionally provide a “meta-tool”:

- `"mcp.call_tool"` with arguments `{ "server": "...", "tool": "...", "args": { ... } }` for generic dispatch, while also registering individual named tools for ergonomics.

---

## 7. Skills System (`roughneck-skills`)

### 7.1 Skill Definitions

Support skill definition files, e.g. `*.skill.toml`:

```toml
name = "rust-best-practices"
description = "Idiomatic Rust patterns for Roughneck"
system_instructions = """
You are a Rust expert. Always:
- Prefer &T over cloning where possible
- Use Result<T, E> instead of panic in production
...
"""

[[tools]]
name = "rust_lint_project"
description = "Lint current Rust project with cargo clippy"
schema = { /* JSON schema */ }
```

`SkillsRegistry`:

- Scans registry paths for skill files.
- Builds `SkillDefinition` structs.
- When a skill is enabled:
  - Appends `system_instructions` to system prompt.
  - Registers associated tools.

---

## 8. Pluggable Memory (`roughneck-memory`)

Implement multiple backends behind `MemoryBackend`:

- `InMemoryMemoryBackend` – good for tests and local dev.
- `SqliteMemoryBackend` – on-disk persistent memory.
- `RedisMemoryBackend` (optional) – for distributed memory.
- Abstract config:

```rust
pub enum MemoryBackendKind {
    InMemory,
    Sqlite { path: PathBuf },
    Redis { url: String },
    Custom(Arc<dyn MemoryBackend>),
}
```

**Policy:**  

- Short-term memory: subset of recent messages in active context window.
- Long-term: retrieved via `search` for RAG-like augmentation (optional) or just contextual reminders.

---

## 9. Language Bindings

### 9.1 Python (`roughneck-py` via PyO3)

API modeled after DeepAgents Python:

```python
from roughneck import create_deep_agent

def get_weather(city: str) -> str:
    return f"It's always sunny in {city}!"

agent = create_deep_agent(
    tools=[get_weather],
    system_prompt="You are a helpful assistant",
    subagents=[ ... ],      # optional
    backend="local_fs",     # or dict config
    skills=["rust-best-practices"],
    memory={"backend": "sqlite", "path": "memory.db"},
)
result = agent.invoke({
    "messages": [{"role": "user", "content": "what is the weather in sf"}]
})
```

Implementation notes:

- Use **PyO3** with `pyo3-asyncio` to bridge async Rust (Tokio) with Python’s event loop.
- Map Python callables as `Tool` implementations by wrapping them in an FFI adapter that:
  - Converts JSON args to Python dict.
  - Calls Python function.
  - Converts result back to JSON/string.

### 9.2 Node/TS (`roughneck-node` via napi-rs)

API modeled after deepagents.js:

```ts
import { createDeepAgent, tool } from "roughneck";

const internetSearch = tool({
  name: "internet_search",
  description: "Run a web search",
  // zod schema or JSON schema
  async run({ query, maxResults = 5 }) {
    // ...
  },
});

const agent = createDeepAgent({
  systemPrompt: "You are an expert researcher.",
  tools: [internetSearch],
  skills: ["web-research"],
  memory: { backend: "in_memory" },
});

const result = await agent.invoke({
  messages: [{ role: "user", content: "What is LangGraph?" }],
});
```

Implementation notes:

- Use **napi-rs** with a global Tokio runtime.
- Map JS tool functions to `Tool` implementations similarly to Python.

---

## 10. Integration with Rig

### 10.1 Model & Provider Abstraction

- Use Rig’s provider clients and models for completions and tool-calling:

```rust
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::providers::openai;

pub enum ModelProviderConfig {
    OpenAi { model: String },
    Anthropic { model: String },
    // etc...
}
```

- `DeepAgentConfig` includes a `model` field referencing Rig providers.
- Under the hood, `DeepAgent` delegates completion/tool-calling to a Rig `Agent` where possible; otherwise, it uses `CompletionModel` traits and manual tool-calling loop.

### 10.2 Vector Stores & RAG (Future Extension)

- Roughneck should be able to optionally hook into Rig’s vector stores (e.g. `rig-qdrant`, `rig-mongodb`) for RAG-style memory or skills.
- This can be layered in later via a `VectorStoreBackend` abstraction.

---

## 11. Configuration & Extensibility

### 11.1 Configuration Sources

- Programmatic: `DeepAgentConfig` builder in Rust.
- Declarative:
  - `roughneck.toml` (optional) for CLI and host language bindings.
  - Python/Node wrappers can load from this file by default.

### 11.2 Extension Points

- New **tool** types: implement `Tool` and register in config.
- New **filesystem** backends: implement `FileSystemBackend`.
- New **memory** backends: implement `MemoryBackend`.
- New **skills**: add skill definition files to skills registry paths.
- New **MCP** servers: add entries to `McpServerConfig`.

---

## 12. Testing & Examples

- Provide example crates under `examples/`:
  - `examples/basic_agent` – single-agent, planning + fs tools.
  - `examples/research_agent` – sub-agents + web search + memory.
  - `examples/python_binding` – Python usage in a small script.
  - `examples/node_binding` – Node usage with TS types.

- Ensure:
  - Unit tests for traits and backends.
  - Integration tests that spin up a simple DeepAgent and verify planning → tool calls → FS writes → memory.

---

## 13. Implementation Priorities (Suggested Order)

1. `roughneck-core` traits & config structs.
2. Minimal `roughneck-fs` in-memory backend + FS tools.
3. Minimal `roughneck-memory` in-memory backend.
4. `roughneck-runtime`:
   - DeepAgentConfig
   - System prompt assembly
   - Tool-calling loop using Rig
   - Planning (`write_todos`)
5. Sub-agent support (`call_subagent` + nested DeepAgents).
6. Skills registry & integration.
7. MCP client + dynamic tools.
8. Python bindings (`roughneck-py`) with `create_deep_agent`.
9. Node bindings (`roughneck-node`) with `createDeepAgent`.
10. CLI harness (`roughneck-cli`).

This should provide enough structure and naming for an AI coding assistant to scaffold Roughneck’s codebase and iteratively fill in the implementation details.


---

## Sources

- [rig - Rust](https://docs.rs/rig-core/latest/rig/)
- [GitHub - 0xPlaygrounds/rig: ⚙️🦀 Build modular and scalable LLM Applications in Rust](https://github.com/0xPlaygrounds/rig)
- [Rig - Build Powerful LLM Applications in Rust](https://rig.rs/)
- [Deep Agents overview - Docs by LangChain](https://docs.langchain.com/oss/python/deepagents/overview)
- [deepagents | LangChain Reference](https://reference.langchain.com/python/deepagents)
- [deepagents - npm](https://www.npmjs.com/package/deepagents)
- [Deep Agents](https://blog.langchain.com/deep-agents/)
- [Rig-rs - Qdrant](https://qdrant.tech/documentation/frameworks/rig-rs/)

