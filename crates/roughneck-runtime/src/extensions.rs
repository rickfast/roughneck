use async_trait::async_trait;
use roughneck_core::{
    CapabilityStatus, FileSystemBackend, FileSystemBackendKind, FileSystemConfig, McpConfig,
    Result, RoughneckError, SubagentConfig, SubagentsConfig,
};
use roughneck_fs::{InMemoryFileSystemBackend, LocalFsBackend};
use roughneck_mcp::{McpCallRequest, McpRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

#[async_trait]
pub trait FileSystemSessionFactory: Send + Sync + Debug {
    async fn create_session(
        &self,
        config: &FileSystemConfig,
        session_id: &str,
    ) -> Result<Arc<dyn FileSystemBackend>>;

    fn allows_initial_files(&self, config: &FileSystemConfig) -> bool;

    fn snapshot_on_response(&self, config: &FileSystemConfig) -> bool;
}

#[derive(Debug, Default)]
pub struct DefaultFileSystemSessionFactory;

#[async_trait]
impl FileSystemSessionFactory for DefaultFileSystemSessionFactory {
    async fn create_session(
        &self,
        config: &FileSystemConfig,
        _session_id: &str,
    ) -> Result<Arc<dyn FileSystemBackend>> {
        match &config.backend {
            FileSystemBackendKind::InMemory => Ok(Arc::new(InMemoryFileSystemBackend::new(
                config.execute.enabled,
            ))),
            FileSystemBackendKind::Local { root } => Ok(Arc::new(LocalFsBackend::new(
                root.clone(),
                config.execute.enabled,
            ))),
        }
    }

    fn allows_initial_files(&self, config: &FileSystemConfig) -> bool {
        matches!(config.backend, FileSystemBackendKind::InMemory)
    }

    fn snapshot_on_response(&self, config: &FileSystemConfig) -> bool {
        config.snapshot_on_response()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentRequest {
    pub session_id: String,
    pub invocation_id: String,
    pub subagent: String,
    pub task: String,
    #[serde(default)]
    pub context_files: Vec<String>,
    pub depth: usize,
}

#[async_trait]
pub trait SubagentRuntime: Send + Sync + Debug {
    async fn invoke(&self, request: SubagentRequest) -> Result<Value>;
}

#[derive(Debug)]
pub struct DefaultSubagentRuntime {
    status: CapabilityStatus,
    configured: HashMap<String, SubagentConfig>,
}

impl DefaultSubagentRuntime {
    #[must_use]
    pub fn new(config: &SubagentsConfig) -> Self {
        Self {
            status: config.status,
            configured: config
                .agents
                .iter()
                .cloned()
                .map(|agent| (agent.name.clone(), agent))
                .collect(),
        }
    }
}

#[async_trait]
impl SubagentRuntime for DefaultSubagentRuntime {
    async fn invoke(&self, request: SubagentRequest) -> Result<Value> {
        let Some(agent) = self.configured.get(&request.subagent) else {
            return Err(RoughneckError::NotFound(format!(
                "unknown subagent {}",
                request.subagent
            )));
        };

        let payload = json!({
            "subagent": agent.name,
            "description": agent.description,
            "task": request.task,
            "context_files": request.context_files,
            "depth": request.depth,
        });

        Ok(match self.status {
            CapabilityStatus::Disabled => json!({
                "status": "unsupported_capability",
                "capability_status": "disabled",
                "message": "Subagent capability is disabled.",
                "request": payload,
            }),
            CapabilityStatus::Experimental => json!({
                "status": "experimental_capability",
                "capability_status": "experimental",
                "message": "Subagent dispatch is wired through the runtime interface, but concrete nested execution is still experimental.",
                "request": payload,
            }),
            CapabilityStatus::Active => json!({
                "status": "unsupported_capability",
                "capability_status": "active",
                "message": "Subagent capability is configured as active, but no concrete SubagentRuntime implementation was installed.",
                "request": payload,
            }),
        })
    }
}

#[async_trait]
pub trait McpClient: Send + Sync + Debug {
    async fn call_tool(&self, request: &McpCallRequest) -> Result<Value>;
}

#[derive(Debug)]
pub struct DefaultMcpClient {
    status: CapabilityStatus,
    registry: Arc<McpRegistry>,
}

impl DefaultMcpClient {
    #[must_use]
    pub fn new(config: &McpConfig, registry: Arc<McpRegistry>) -> Self {
        Self {
            status: config.status,
            registry,
        }
    }
}

#[async_trait]
impl McpClient for DefaultMcpClient {
    async fn call_tool(&self, request: &McpCallRequest) -> Result<Value> {
        let spec = self.registry.validate_call(request)?;

        Ok(match self.status {
            CapabilityStatus::Disabled => json!({
                "status": "unsupported_capability",
                "capability_status": "disabled",
                "message": "MCP capability is disabled.",
                "server": request.server,
                "tool": request.tool,
            }),
            CapabilityStatus::Experimental => json!({
                "status": "experimental_capability",
                "capability_status": "experimental",
                "message": "MCP dispatch is routed through the client interface, but live transport is still experimental.",
                "server": request.server,
                "tool": request.tool,
                "tool_registered": spec.is_some(),
                "args": request.args,
            }),
            CapabilityStatus::Active => json!({
                "status": "unsupported_capability",
                "capability_status": "active",
                "message": "MCP capability is configured as active, but no concrete McpClient implementation was installed.",
                "server": request.server,
                "tool": request.tool,
                "tool_registered": spec.is_some(),
                "args": request.args,
            }),
        })
    }
}
