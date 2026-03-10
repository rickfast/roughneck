use crate::extensions::{McpClient, SubagentRequest, SubagentRuntime};
use crate::hooks::{HookCapture, HookContext, HookManager};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use roughneck_core::{
    CapabilityStatus, McpConfig, MemoryBackend, MemoryEvent, MemoryScope, RoughneckError, TodoItem,
    now_millis,
};
use roughneck_mcp::{McpCallRequest, McpRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug)]
pub struct WriteTodosTool {
    store: Arc<tokio::sync::RwLock<Vec<TodoItem>>>,
    memory: Arc<dyn MemoryBackend>,
    session_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WriteTodosArgs {
    #[serde(default)]
    pub todos: Vec<TodoItem>,
}

impl WriteTodosTool {
    pub fn new(
        store: Arc<tokio::sync::RwLock<Vec<TodoItem>>>,
        memory: Arc<dyn MemoryBackend>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            memory,
            session_id: session_id.into(),
        }
    }
}

impl Tool for WriteTodosTool {
    const NAME: &'static str = "write_todos";
    type Error = RoughneckError;
    type Args = WriteTodosArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Record or update a todo list for the current session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task": {"type": "string"},
                                "status": {"type": "string", "enum": ["pending", "done"]}
                            },
                            "required": ["task", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        *self.store.write().await = args.todos.clone();
        self.memory
            .append_event(
                &self.session_id,
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "todo_update".to_string(),
                    payload: serde_json::to_value(&args.todos)?,
                    timestamp_ms: now_millis(),
                },
            )
            .await?;
        Ok(json!({"recorded": args.todos.len(), "todos": args.todos}))
    }
}

#[derive(Debug)]
pub struct CallSubagentTool {
    status: CapabilityStatus,
    runtime: Arc<dyn SubagentRuntime>,
    hooks: Arc<HookManager>,
    hook_capture: Arc<HookCapture>,
    session_id: String,
    invocation_id: String,
    depth: usize,
    max_depth: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CallSubagentRuntimeContext {
    pub(crate) hooks: Arc<HookManager>,
    pub(crate) hook_capture: Arc<HookCapture>,
    pub(crate) session_id: String,
    pub(crate) invocation_id: String,
    pub(crate) depth: usize,
    pub(crate) max_depth: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CallSubagentArgs {
    pub subagent: String,
    pub task: String,
    #[serde(default)]
    pub context_files: Vec<String>,
}

impl CallSubagentTool {
    pub fn new(
        status: CapabilityStatus,
        runtime: Arc<dyn SubagentRuntime>,
        context: CallSubagentRuntimeContext,
    ) -> Self {
        Self {
            status,
            runtime,
            hooks: context.hooks,
            hook_capture: context.hook_capture,
            session_id: context.session_id,
            invocation_id: context.invocation_id,
            depth: context.depth,
            max_depth: context.max_depth,
        }
    }
}

impl Tool for CallSubagentTool {
    const NAME: &'static str = "call_subagent";
    type Error = RoughneckError;
    type Args = CallSubagentArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Delegate to a configured subagent via the runtime interface. Current capability status: {:?}.",
                self.status
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "subagent": {"type": "string"},
                    "task": {"type": "string"},
                    "context_files": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["subagent", "task"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        if self.depth >= self.max_depth {
            return Err(RoughneckError::Runtime(format!(
                "subagent recursion limit exceeded at depth {}",
                self.depth
            )));
        }

        let request = SubagentRequest {
            session_id: self.session_id.clone(),
            invocation_id: self.invocation_id.clone(),
            subagent: args.subagent,
            task: args.task,
            context_files: args.context_files,
            depth: self.depth + 1,
        };

        let result = self.runtime.invoke(request).await?;
        let hook_ctx = HookContext::new(self.session_id.clone(), self.invocation_id.clone(), None);
        let decision = self
            .hooks
            .subagent_stop(&hook_ctx, "subagent_finished", Some(&result))
            .await?;
        self.hook_capture.record(&decision).await;
        if decision.blocked {
            return Err(RoughneckError::Runtime(
                decision
                    .reason
                    .unwrap_or_else(|| "blocked by SubagentStop hook".to_string()),
            ));
        }

        Ok(result)
    }
}

#[derive(Debug)]
pub struct McpMetaTool {
    status: CapabilityStatus,
    registry: Arc<McpRegistry>,
    client: Arc<dyn McpClient>,
    server_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct McpMetaArgs {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

impl McpMetaTool {
    pub fn from_config(
        config: &McpConfig,
        registry: Arc<McpRegistry>,
        client: Arc<dyn McpClient>,
    ) -> Self {
        Self {
            status: config.status,
            registry,
            client,
            server_count: config.servers.len(),
        }
    }
}

impl Tool for McpMetaTool {
    const NAME: &'static str = "mcp.call_tool";
    type Error = RoughneckError;
    type Args = McpMetaArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Dispatch an MCP tool call through the MCP client interface. Current capability status: {:?}. Configured servers: {}.",
                self.status, self.server_count,
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string"},
                    "tool": {"type": "string"},
                    "args": {"type": "object"}
                },
                "required": ["server", "tool"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        self.registry.validate_server(&args.server)?;
        self.client
            .call_tool(&McpCallRequest {
                server: args.server,
                tool: args.tool,
                args: args.args,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roughneck_memory::InMemoryMemoryBackend;

    #[tokio::test]
    async fn write_todos_updates_store() {
        let store = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let tool = WriteTodosTool::new(
            store.clone(),
            Arc::new(InMemoryMemoryBackend::default()),
            "session",
        );

        rig::tool::Tool::call(
            &tool,
            WriteTodosArgs {
                todos: vec![TodoItem {
                    task: "Use rig tools".to_string(),
                    status: roughneck_core::TodoStatus::Pending,
                }],
            },
        )
        .await
        .unwrap();

        let todos = store.read().await;
        assert_eq!(todos.len(), 1);
    }
}
