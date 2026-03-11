# Roughneck

Roughneck is a Rust-first deep-agent harness built on top of [Rig](https://github.com/0xPlaygrounds/rig).

## Current status

- Rig `Agent` and model clients are used directly in runtime (`openai` and `anthropic`).
- Runtime state is session-oriented: `DeepAgent::start_session(...)` creates an `AgentSession`, and `AgentSession::invoke(...)` replays prior chat turns from `MemoryBackend`.
- Filesystem state is per session. In-memory sessions can be seeded with `initial_files`; local filesystem sessions operate directly on the configured root and do not expose overlay seeding.
- Skills can be loaded from `*.skill.toml` and Markdown skill files such as `SKILL.md`.
- Hook extension points are available for `PreToolUse`, `PostToolUse`, `Notification`, `Stop`, and `SubagentStop`, with real session and invocation context.
- Programmatic tools can be registered from Rust, Python, and Node / TypeScript.
- Host bindings now exist as:
  - `roughneck-py` using PyO3
  - `roughneck-node` using napi-rs
- The bindings ship inline type artifacts:
  - Python: `crates/roughneck-py/python/roughneck_py/__init__.pyi`
  - Node / TypeScript: `crates/roughneck-node/index.d.ts` and `crates/roughneck-node/roughneck.node.d.ts`

## Quickstart

```bash
cargo test --workspace --exclude roughneck-py --exclude roughneck-node
cargo check -p roughneck-py
cargo check -p roughneck-node
cargo run -p roughneck-cli -- --provider openai --model gpt-4o-mini --prompt "List files"
cargo run --manifest-path examples/basic_agent/Cargo.toml -- "Summarize the seeded workspace"
```

Set provider API keys before running:

- `OPENAI_API_KEY` for OpenAI
- `ANTHROPIC_API_KEY` for Anthropic

## Rust API

```rust
use roughneck_core::{ChatMessage, RoughneckError, SessionInit, SessionInvokeRequest};
use roughneck_runtime::DeepAgent;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LookupArgs {
    name: String,
}

#[derive(Debug, Clone)]
struct LookupReleaseTool;

impl Tool for LookupReleaseTool {
    const NAME: &'static str = "lookup_release";
    type Error = RoughneckError;
    type Args = LookupArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Return a canned release version.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({ "name": args.name, "version": "0.1.0" }))
    }
}

let agent = DeepAgent::new(Default::default())?.with_tool(LookupReleaseTool);
let session = agent.start_session(SessionInit::default()).await?;
let response = session
    .invoke(SessionInvokeRequest {
        messages: vec![ChatMessage::user("Summarize the workspace")],
    })
    .await?;
```

## Python Binding

The Python module includes a `.pyi` stub. Config, request, response, and hook payload keys use `snake_case` to match the Rust serde contract directly.

Packaging and smoke tests:

```bash
cd crates/roughneck-py
maturin develop
PYTHONPATH=python python3 tests/runtime_smoke.py
python3 -m mypy tests/typecheck_smoke.py
```

```python
import roughneck_py

agent = roughneck_py.create_deep_agent({})

def audit(payload):
    print("python hook:", payload["hook_event_name"])
    return None

agent.register_hook("notification", audit)
agent.register_tool(
    "lookup_release",
    "Return a canned release version for a package name.",
    {
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
    },
    lambda payload: {"version": "0.1.0", "name": payload["name"]},
)

session = agent.start_session({})
response = session.invoke({"messages": [{"role": "user", "content": "Summarize the workspace"}]})
print(response["latest_assistant_message"]["content"])
```

## Node / TypeScript Binding

The Node binding includes `.d.ts` declarations. Method names are JavaScript-style (`registerHook`, `startSession`), while config, request, response, and hook payload objects still use `snake_case` keys because they are passed through to Rust as JSON.

Packaging and smoke tests:

```bash
cd crates/roughneck-node
npm install
npm run build
npm run smoke:runtime
npm run smoke:types
```

```ts
import { createDeepAgent } from './roughneck.node'

const agent = createDeepAgent({})

agent.registerHook('notification', (payload) => {
  console.log('node hook:', payload.hook_event_name)
  return undefined
})
agent.registerTool(
  'lookup_release',
  'Return a canned release version for a package name.',
  {
    type: 'object',
    properties: { name: { type: 'string' } },
    required: ['name'],
  },
  (input) => {
    if (input && typeof input === 'object' && !Array.isArray(input)) {
      return { version: '0.1.0', name: input['name'] ?? null }
    }
    return null
  },
)

const session = await agent.startSession({})
const response = await session.invoke({
  messages: [{ role: 'user', content: 'Summarize the workspace' }],
})
console.log(response.latest_assistant_message?.content)
```

## Hooks

Hooks are configured in `roughneck.toml` under `[hooks]` and `[[hooks.<event>]]` lists.
Each hook command receives JSON on stdin with `session_id`, `invocation_id`, optional `tool_call_id`, and event-specific payload. Hooks can:

- allow: exit `0`
- block: either return JSON `{ "decision": "block", "reason": "..." }` or exit `2`
- suppress tool output: return JSON `{ "suppress_output": true }`
- attach messages/output to the session response: return JSON with `messages` and `hook_specific_output`

The Python and Node bindings also support in-process hook callbacks:

- Python: `agent.register_hook("pre_tool_use", callback)`
- Node: `agent.registerHook("preToolUse", callback)`

Binding callbacks currently expect synchronous functions that receive the same JSON payload shape as shell hooks and return either `None` / `undefined` or a `HookDecision`-shaped object.

See [roughneck.toml.example](./roughneck.toml.example) for sample configuration.
