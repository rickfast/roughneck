use roughneck_core::{Result, RoughneckError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolSpec {
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCallRequest {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Default)]
pub struct McpRegistry {
    servers: HashSet<String>,
    tools: HashMap<(String, String), McpToolSpec>,
}

impl McpRegistry {
    /// Creates an empty MCP registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: HashSet::new(),
            tools: HashMap::new(),
        }
    }

    pub fn register_server(&mut self, server: impl Into<String>) {
        self.servers.insert(server.into());
    }

    /// Validates that a named MCP server is registered.
    ///
    /// # Errors
    ///
    /// Returns an error if the server is unknown.
    pub fn validate_server(&self, server: &str) -> Result<()> {
        if self.servers.contains(server) {
            Ok(())
        } else {
            Err(RoughneckError::NotFound(format!(
                "unknown MCP server {server}"
            )))
        }
    }

    pub fn register(&mut self, spec: McpToolSpec) {
        self.register_server(spec.server.clone());
        self.tools
            .insert((spec.server.clone(), spec.name.clone()), spec);
    }

    /// Returns the registered MCP tools in stable sorted order.
    #[must_use]
    pub fn list_tools(&self) -> Vec<McpToolSpec> {
        let mut out: Vec<McpToolSpec> = self.tools.values().cloned().collect();
        out.sort_by(|a, b| a.server.cmp(&b.server).then(a.name.cmp(&b.name)));
        out
    }

    /// Validates an MCP tool call against the registered servers and tools.
    ///
    /// # Errors
    ///
    /// Returns an error if the target server is unknown.
    pub fn validate_call(&self, request: &McpCallRequest) -> Result<Option<&McpToolSpec>> {
        self.validate_server(&request.server)?;
        Ok(self
            .tools
            .get(&(request.server.clone(), request.tool.clone())))
    }
}
