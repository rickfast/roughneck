mod extensions;
mod hooks;
mod tools;

pub use extensions::{
    DefaultFileSystemSessionFactory, DefaultMcpClient, DefaultSubagentRuntime,
    FileSystemSessionFactory, McpClient, SubagentRequest, SubagentRuntime,
};
pub use hooks::{
    HookCapture, HookContext, HookDecision, HookEvent, HookExecutor, HookManager, HookPayload,
    ToolRuntimeContext,
};

use rig::client::CompletionClient;
use rig::completion::{Prompt, PromptError};
use rig::providers::{anthropic, openai};
use rig::tool::{Tool, ToolDyn};
use roughneck_core::{
    CapabilityStatus, ChatMessage, DeepAgentConfig, FileSystemBackend, HookOutputSummary,
    MemoryBackend, MemoryBackendKind, MemoryEvent, MemoryScope, ModelProviderConfig, Result, Role,
    RoughneckError, SessionInit, SessionInvokeRequest, SessionInvokeResponse, TodoItem, now_millis,
};
use roughneck_mcp::McpRegistry;
use roughneck_memory::InMemoryMemoryBackend;
use roughneck_skills::SkillsRegistry;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tools::{CallSubagentRuntimeContext, CallSubagentTool, McpMetaTool, WriteTodosTool};
use uuid::Uuid;

#[derive(Debug, Clone)]
#[must_use]
pub struct DeepAgent {
    inner: Arc<DeepAgentInner>,
}

#[derive(Debug)]
struct DeepAgentInner {
    config: DeepAgentConfig,
    memory: Arc<dyn MemoryBackend>,
    skills_prompt: String,
    hooks: Arc<HookManager>,
    filesystem_factory: Arc<dyn FileSystemSessionFactory>,
    subagent_runtime: Arc<dyn SubagentRuntime>,
    mcp_registry: Arc<McpRegistry>,
    mcp_client: Arc<dyn McpClient>,
}

#[derive(Debug, Clone)]
pub struct AgentSession {
    inner: Arc<DeepAgentInner>,
    session_id: String,
    fs: Arc<dyn FileSystemBackend>,
    todos: Arc<tokio::sync::RwLock<Vec<TodoItem>>>,
    depth: usize,
}

impl DeepAgent {
    /// Creates a new deep-agent runtime from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if skills or hooks cannot be initialized from the provided configuration.
    pub fn new(config: DeepAgentConfig) -> Result<Self> {
        let memory = make_memory(&config);
        let skills = if config.skills.enabled_skills.is_empty() {
            Vec::new()
        } else {
            SkillsRegistry::from_config(&config.skills)?
        };
        let skills_prompt = SkillsRegistry::prompt_section(&skills);
        let hooks = Arc::new(HookManager::new(config.hooks.clone())?);
        let filesystem_factory: Arc<dyn FileSystemSessionFactory> =
            Arc::new(DefaultFileSystemSessionFactory);
        let mcp_registry = Arc::new(make_mcp_registry(&config));
        let subagent_runtime: Arc<dyn SubagentRuntime> =
            Arc::new(DefaultSubagentRuntime::new(&config.subagents));
        let mcp_client: Arc<dyn McpClient> =
            Arc::new(DefaultMcpClient::new(&config.mcp, mcp_registry.clone()));

        Ok(Self {
            inner: Arc::new(DeepAgentInner {
                config,
                memory,
                skills_prompt,
                hooks,
                filesystem_factory,
                subagent_runtime,
                mcp_registry,
                mcp_client,
            }),
        })
    }

    /// Replaces the memory backend for subsequently created sessions.
    ///
    /// # Panics
    ///
    /// Panics if the agent has been cloned and no longer has unique ownership of its inner state.
    pub fn with_memory(mut self, memory: Arc<dyn MemoryBackend>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("DeepAgent::with_memory requires unique ownership")
            .memory = memory;
        self
    }

    /// Replaces the filesystem session factory for subsequently created sessions.
    ///
    /// # Panics
    ///
    /// Panics if the agent has been cloned and no longer has unique ownership of its inner state.
    pub fn with_filesystem_factory(mut self, factory: Arc<dyn FileSystemSessionFactory>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("DeepAgent::with_filesystem_factory requires unique ownership")
            .filesystem_factory = factory;
        self
    }

    /// Replaces the subagent runtime implementation.
    ///
    /// # Panics
    ///
    /// Panics if the agent has been cloned and no longer has unique ownership of its inner state.
    pub fn with_subagent_runtime(mut self, runtime: Arc<dyn SubagentRuntime>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("DeepAgent::with_subagent_runtime requires unique ownership")
            .subagent_runtime = runtime;
        self
    }

    /// Replaces the MCP client implementation.
    ///
    /// # Panics
    ///
    /// Panics if the agent has been cloned and no longer has unique ownership of its inner state.
    pub fn with_mcp_client(mut self, client: Arc<dyn McpClient>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("DeepAgent::with_mcp_client requires unique ownership")
            .mcp_client = client;
        self
    }

    /// Attaches an in-process hook executor.
    ///
    /// # Panics
    ///
    /// Panics if the agent has been cloned and no longer has unique ownership of its inner state.
    pub fn with_hook_executor(mut self, executor: Arc<dyn HookExecutor>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("DeepAgent::with_hook_executor requires unique ownership")
            .hooks = Arc::new(self.inner.hooks.with_executor(executor));
        self
    }

    /// Starts a new isolated agent session.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem session cannot be created or initial state cannot be seeded.
    pub async fn start_session(&self, init: SessionInit) -> Result<AgentSession> {
        let session_id = init
            .session_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let fs = self
            .inner
            .filesystem_factory
            .create_session(&self.inner.config.filesystem, &session_id)
            .await?;

        if !init.initial_files.is_empty()
            && !self
                .inner
                .filesystem_factory
                .allows_initial_files(&self.inner.config.filesystem)
        {
            return Err(RoughneckError::InvalidInput(
                "initial_files are only supported for in-memory filesystem sessions".to_string(),
            ));
        }

        for (path, content) in init.initial_files {
            fs.write_file(&path, &content).await?;
        }

        append_chat_messages(&self.inner.memory, &session_id, &init.initial_messages).await?;

        Ok(AgentSession {
            inner: self.inner.clone(),
            session_id,
            fs,
            todos: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            depth: 0,
        })
    }
}

impl AgentSession {
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Invokes the session with the next user turn.
    ///
    /// # Errors
    ///
    /// Returns an error if hooks, memory replay, model invocation, or response persistence fails.
    pub async fn invoke(&self, req: SessionInvokeRequest) -> Result<SessionInvokeResponse> {
        if req.messages.is_empty() {
            return Err(RoughneckError::InvalidInput(
                "invoke requires at least one message".to_string(),
            ));
        }

        let invocation_id = Uuid::new_v4().to_string();
        let hook_capture = Arc::new(HookCapture::default());
        let invocation_ctx = HookContext::new(self.session_id.clone(), invocation_id.clone(), None);

        append_chat_messages(&self.inner.memory, &self.session_id, &req.messages).await?;

        let replay_messages = load_chat_history(
            &self.inner.memory,
            &self.session_id,
            self.inner.config.memory.short_term_limit.max(1),
        )
        .await?;
        let prompt = build_prompt(&replay_messages);
        let preamble = self.build_preamble();

        let notification = self
            .inner
            .hooks
            .notification(
                &invocation_ctx,
                "invoke_started",
                Some(&json!({"message_count": req.messages.len()})),
            )
            .await?;
        hook_capture.record(&notification).await;
        if notification.blocked {
            persist_hook_summary(&self.inner.memory, &self.session_id, &hook_capture).await?;
            return Err(RoughneckError::Runtime(
                notification
                    .reason
                    .unwrap_or_else(|| "blocked by Notification hook".to_string()),
            ));
        }

        let tool_runtime = Arc::new(ToolRuntimeContext {
            session_id: self.session_id.clone(),
            invocation_id: invocation_id.clone(),
            memory: self.inner.memory.clone(),
            hook_capture: hook_capture.clone(),
            tool_call_counter: Arc::new(AtomicUsize::new(0)),
        });

        let answer = self
            .prompt_with_rig(
                &preamble,
                prompt,
                self.build_tools(tool_runtime, hook_capture.clone(), &invocation_id),
            )
            .await;
        let answer = match answer {
            Ok(answer) => answer,
            Err(err) => {
                persist_hook_summary(&self.inner.memory, &self.session_id, &hook_capture).await?;
                return Err(err);
            }
        };

        self.finalize_invoke(&invocation_ctx, answer, &hook_capture)
            .await
    }

    fn build_tools(
        &self,
        tool_runtime: Arc<ToolRuntimeContext>,
        hook_capture: Arc<HookCapture>,
        invocation_id: &str,
    ) -> Vec<Box<dyn ToolDyn>> {
        let mut tools: Vec<Box<dyn ToolDyn>> = vec![
            hook(
                roughneck_fs::LsTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::ReadFileTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::WriteFileTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::EditFileTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::GlobTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::GrepTool::new(self.fs.clone()),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                roughneck_fs::ExecuteTool::new(
                    self.fs.clone(),
                    self.inner.config.filesystem.execute.default_timeout_secs,
                    self.inner.config.filesystem.execute.max_timeout_secs,
                ),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
            hook(
                WriteTodosTool::new(
                    self.todos.clone(),
                    self.inner.memory.clone(),
                    self.session_id.clone(),
                ),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ),
        ];

        if subagent_tool_enabled(&self.inner.config) {
            tools.push(hook(
                CallSubagentTool::new(
                    self.inner.config.subagents.status,
                    self.inner.subagent_runtime.clone(),
                    CallSubagentRuntimeContext {
                        hooks: self.inner.hooks.clone(),
                        hook_capture,
                        session_id: self.session_id.clone(),
                        invocation_id: invocation_id.to_string(),
                        depth: self.depth,
                        max_depth: self.inner.config.subagents.max_depth,
                    },
                ),
                self.inner.hooks.clone(),
                tool_runtime.clone(),
            ));
        }

        if mcp_tool_enabled(&self.inner.config) {
            tools.push(hook(
                McpMetaTool::from_config(
                    &self.inner.config.mcp,
                    self.inner.mcp_registry.clone(),
                    self.inner.mcp_client.clone(),
                ),
                self.inner.hooks.clone(),
                tool_runtime,
            ));
        }

        tools
    }

    fn build_preamble(&self) -> String {
        format!(
            "{}\n\n{}{}",
            build_harness_instructions(&self.inner.config),
            self.inner.config.system_prompt,
            self.inner.skills_prompt,
        )
    }

    fn build_metadata(
        &self,
        hook_output: &HookOutputSummary,
        workspace_snapshot_included: bool,
    ) -> HashMap<String, Value> {
        HashMap::from([
            (
                "provider".to_string(),
                json!(match &self.inner.config.model {
                    ModelProviderConfig::OpenAi { .. } => "openai",
                    ModelProviderConfig::Anthropic { .. } => "anthropic",
                }),
            ),
            ("max_turns".to_string(), json!(self.inner.config.max_turns)),
            (
                "hooks_enabled".to_string(),
                json!(self.inner.hooks.is_active()),
            ),
            (
                "workspace_snapshot_included".to_string(),
                json!(workspace_snapshot_included),
            ),
            (
                "capabilities".to_string(),
                json!({
                    "subagents": {
                        "status": self.inner.config.subagents.status,
                        "configured": self.inner.config.subagents.agents.len(),
                        "max_depth": self.inner.config.subagents.max_depth,
                    },
                    "mcp": {
                        "status": self.inner.config.mcp.status,
                        "servers": self.inner.config.mcp.servers.len(),
                        "meta_tool_enabled": self.inner.config.mcp.enable_meta_tool,
                    }
                }),
            ),
            ("hooks".to_string(), json!(hook_output)),
        ])
    }

    async fn finalize_invoke(
        &self,
        invocation_ctx: &HookContext,
        answer: String,
        hook_capture: &Arc<HookCapture>,
    ) -> Result<SessionInvokeResponse> {
        let stop = self
            .inner
            .hooks
            .stop(
                invocation_ctx,
                "assistant_response_ready",
                Some(&json!({"chars": answer.len()})),
            )
            .await?;
        hook_capture.record(&stop).await;
        if stop.blocked {
            persist_hook_summary(&self.inner.memory, &self.session_id, hook_capture).await?;
            return Err(RoughneckError::Runtime(
                stop.reason
                    .unwrap_or_else(|| "blocked by Stop hook".to_string()),
            ));
        }

        let assistant = ChatMessage::assistant(answer);
        append_chat_messages(
            &self.inner.memory,
            &self.session_id,
            std::slice::from_ref(&assistant),
        )
        .await?;

        let hook_output =
            persist_hook_summary(&self.inner.memory, &self.session_id, hook_capture).await?;
        let workspace_snapshot = if self
            .inner
            .filesystem_factory
            .snapshot_on_response(&self.inner.config.filesystem)
        {
            Some(self.fs.snapshot().await?)
        } else {
            None
        };
        let todos = self.todos.read().await.clone();
        let metadata = self.build_metadata(&hook_output, workspace_snapshot.is_some());

        Ok(SessionInvokeResponse {
            session_id: self.session_id.clone(),
            new_messages: vec![assistant.clone()],
            latest_assistant_message: Some(assistant),
            workspace_snapshot,
            todos,
            hook_output,
            metadata,
        })
    }

    async fn prompt_with_rig(
        &self,
        preamble: &str,
        prompt: String,
        tools: Vec<Box<dyn ToolDyn>>,
    ) -> Result<String> {
        match &self.inner.config.model {
            ModelProviderConfig::OpenAi { model, api_key_env } => {
                let env_name = if api_key_env.is_empty() {
                    "OPENAI_API_KEY"
                } else {
                    api_key_env.as_str()
                };
                let api_key = std::env::var(env_name).map_err(|_| {
                    RoughneckError::Config(format!("missing OpenAI API key in env var {env_name}"))
                })?;

                let client = openai::Client::new(&api_key)
                    .map_err(|err| RoughneckError::Config(err.to_string()))?;
                let mut builder = client.agent(model).preamble(preamble).tools(tools);
                if let Some(max_tokens) = self.inner.config.max_tokens {
                    builder = builder.max_tokens(max_tokens);
                }
                let agent = builder.build();

                agent
                    .prompt(prompt)
                    .max_turns(self.inner.config.max_turns)
                    .await
                    .map_err(|err: PromptError| RoughneckError::Runtime(err.to_string()))
            }
            ModelProviderConfig::Anthropic { model, api_key_env } => {
                let env_name = if api_key_env.is_empty() {
                    "ANTHROPIC_API_KEY"
                } else {
                    api_key_env.as_str()
                };
                let api_key = std::env::var(env_name).map_err(|_| {
                    RoughneckError::Config(format!(
                        "missing Anthropic API key in env var {env_name}"
                    ))
                })?;

                let client = anthropic::Client::new(&api_key)
                    .map_err(|err| RoughneckError::Config(err.to_string()))?;
                let mut builder = client.agent(model).preamble(preamble).tools(tools);
                if let Some(max_tokens) = self.inner.config.max_tokens {
                    builder = builder.max_tokens(max_tokens);
                }
                let agent = builder.build();

                agent
                    .prompt(prompt)
                    .max_turns(self.inner.config.max_turns)
                    .await
                    .map_err(|err: PromptError| RoughneckError::Runtime(err.to_string()))
            }
        }
    }
}

fn hook<T>(tool: T, hooks: Arc<HookManager>, runtime: Arc<ToolRuntimeContext>) -> Box<dyn ToolDyn>
where
    T: Tool<Error = RoughneckError, Output = Value> + Send + Sync + 'static,
    T::Args: Serialize + Send + Sync,
{
    Box::new(hooks::HookedTool::new(tool, hooks, runtime))
}

async fn append_chat_messages(
    memory: &Arc<dyn MemoryBackend>,
    session_id: &str,
    messages: &[ChatMessage],
) -> Result<()> {
    for message in messages {
        memory
            .append_event(
                session_id,
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "chat_message".to_string(),
                    payload: serde_json::to_value(message)?,
                    timestamp_ms: now_millis(),
                },
            )
            .await?;
    }
    Ok(())
}

async fn load_chat_history(
    memory: &Arc<dyn MemoryBackend>,
    session_id: &str,
    short_term_limit: usize,
) -> Result<Vec<ChatMessage>> {
    let mut messages = memory
        .get_events(session_id, usize::MAX)
        .await?
        .into_iter()
        .filter(|event| event.kind == "chat_message")
        .filter_map(|event| serde_json::from_value::<ChatMessage>(event.payload).ok())
        .collect::<Vec<_>>();

    if messages.len() > short_term_limit {
        let start = messages.len() - short_term_limit;
        messages = messages.split_off(start);
    }

    Ok(messages)
}

async fn persist_hook_summary(
    memory: &Arc<dyn MemoryBackend>,
    session_id: &str,
    hook_capture: &HookCapture,
) -> Result<HookOutputSummary> {
    let summary = hook_capture.snapshot().await;

    for message in &summary.messages {
        memory
            .append_event(
                session_id,
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "hook_message".to_string(),
                    payload: json!({"message": message}),
                    timestamp_ms: now_millis(),
                },
            )
            .await?;
    }

    for output in &summary.outputs {
        memory
            .append_event(
                session_id,
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "hook_output".to_string(),
                    payload: output.clone(),
                    timestamp_ms: now_millis(),
                },
            )
            .await?;
    }

    Ok(summary)
}

fn make_memory(config: &DeepAgentConfig) -> Arc<dyn MemoryBackend> {
    match &config.memory.backend {
        MemoryBackendKind::InMemory => Arc::new(InMemoryMemoryBackend::default()),
    }
}

fn make_mcp_registry(config: &DeepAgentConfig) -> McpRegistry {
    let mut registry = McpRegistry::new();
    for server in &config.mcp.servers {
        registry.register_server(server.name.clone());
    }
    registry
}

fn build_prompt(messages: &[ChatMessage]) -> String {
    if messages.is_empty() {
        return "Current user request:\n".to_string();
    }

    let history_len = messages.len().saturating_sub(1);
    let history = &messages[..history_len];

    let mut prompt = String::new();
    if !history.is_empty() {
        prompt.push_str("Conversation context:\n");
        for message in history {
            let role = match message.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            prompt.push_str("- ");
            prompt.push_str(role);
            prompt.push_str(": ");
            prompt.push_str(&message.content);
            prompt.push('\n');
        }
        prompt.push('\n');
    }

    let current = messages
        .last()
        .map_or("", |message| message.content.as_str());
    prompt.push_str("Current user request:\n");
    prompt.push_str(current);
    prompt
}

fn subagent_tool_enabled(config: &DeepAgentConfig) -> bool {
    config.subagents.status != CapabilityStatus::Disabled && !config.subagents.agents.is_empty()
}

fn mcp_tool_enabled(config: &DeepAgentConfig) -> bool {
    config.mcp.status != CapabilityStatus::Disabled
        && config.mcp.enable_meta_tool
        && !config.mcp.servers.is_empty()
}

fn build_harness_instructions(config: &DeepAgentConfig) -> String {
    let mut lines = vec![
        "You are Roughneck, a deep-agent harness built on Rig.".to_string(),
        String::new(),
        "Available tools in this session:".to_string(),
        "- Planning: `write_todos`".to_string(),
        "- Filesystem: `ls`, `read_file`, `write_file`, `edit_file`, `glob`, `grep`, `execute`"
            .to_string(),
    ];

    if subagent_tool_enabled(config) {
        lines.push(format!(
            "- Delegation: `call_subagent` ({})",
            capability_label(config.subagents.status)
        ));
    }

    if mcp_tool_enabled(config) {
        lines.push(format!(
            "- MCP: `mcp.call_tool` ({})",
            capability_label(config.mcp.status)
        ));
    }

    lines.push(
        "Use tools deliberately. If a capability is experimental, say so explicitly.".to_string(),
    );
    lines.join("\n")
}

fn capability_label(status: CapabilityStatus) -> &'static str {
    match status {
        CapabilityStatus::Disabled => "disabled",
        CapabilityStatus::Experimental => "experimental",
        CapabilityStatus::Active => "active",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_builder_carries_context() {
        let prompt = build_prompt(&[
            ChatMessage::user("first"),
            ChatMessage::assistant("second"),
            ChatMessage::user("third"),
        ]);
        assert!(prompt.contains("second"));
        assert!(prompt.contains("third"));
    }

    #[tokio::test]
    async fn sessions_do_not_share_in_memory_files() {
        let agent = DeepAgent::new(DeepAgentConfig::default()).unwrap();
        let session_a = agent
            .start_session(SessionInit {
                initial_files: HashMap::from([("a.txt".to_string(), "one".to_string())]),
                ..SessionInit::default()
            })
            .await
            .unwrap();
        let session_b = agent.start_session(SessionInit::default()).await.unwrap();

        assert_eq!(session_a.fs.read_file("a.txt", None).await.unwrap(), "one");
        assert!(session_b.fs.read_file("a.txt", None).await.is_err());
    }

    #[tokio::test]
    async fn local_filesystem_rejects_initial_files() {
        let mut config = DeepAgentConfig::default();
        config.filesystem.backend = roughneck_core::FileSystemBackendKind::Local {
            root: std::env::temp_dir(),
        };

        let agent = DeepAgent::new(config).unwrap();
        let err = agent
            .start_session(SessionInit {
                initial_files: HashMap::from([("a.txt".to_string(), "one".to_string())]),
                ..SessionInit::default()
            })
            .await
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("initial_files are only supported for in-memory filesystem sessions")
        );
    }

    #[tokio::test]
    async fn history_is_loaded_from_memory() {
        let agent = DeepAgent::new(DeepAgentConfig::default()).unwrap();
        let session = agent
            .start_session(SessionInit {
                initial_messages: vec![ChatMessage::user("first")],
                session_id: Some("history-test".to_string()),
                ..SessionInit::default()
            })
            .await
            .unwrap();

        append_chat_messages(
            &session.inner.memory,
            session.session_id(),
            &[ChatMessage::assistant("second")],
        )
        .await
        .unwrap();

        let history = load_chat_history(&session.inner.memory, session.session_id(), 8)
            .await
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "first");
        assert_eq!(history[1].content, "second");
    }
}
