use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolDyn};
use roughneck_core::{FilePatch, FileSystemBackend, RoughneckError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub struct LsTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LsArgs {
    #[serde(default)]
    pub path: String,
}

impl LsTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for LsTool {
    const NAME: &'static str = "ls";
    type Error = RoughneckError;
    type Args = LsArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List files and directories at a path.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}}
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let path = if args.path.is_empty() {
            "."
        } else {
            &args.path
        };
        let entries = self.fs.ls(path).await?;
        Ok(json!({"entries": entries}))
    }
}

#[derive(Debug)]
pub struct ReadFileTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReadFileArgs {
    pub path: String,
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
}

impl ReadFileTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";
    type Error = RoughneckError;
    type Args = ReadFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read a file from the workspace.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "start": {"type": "integer", "minimum": 1},
                    "end": {"type": "integer", "minimum": 1}
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let range = match (args.start, args.end) {
            (Some(start), Some(end)) => Some(roughneck_core::LineRange { start, end }),
            _ => None,
        };
        let content = self.fs.read_file(&args.path, range).await?;
        Ok(json!({"path": args.path, "content": content}))
    }
}

#[derive(Debug)]
pub struct WriteFileTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

impl WriteFileTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for WriteFileTool {
    const NAME: &'static str = "write_file";
    type Error = RoughneckError;
    type Args = WriteFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write content to a file.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        self.fs.write_file(&args.path, &args.content).await?;
        Ok(json!({"ok": true, "path": args.path}))
    }
}

#[derive(Debug)]
pub struct EditFileTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EditFileArgs {
    pub path: String,
    pub search: String,
    pub replace: String,
    #[serde(default)]
    pub replace_all: bool,
}

impl EditFileTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";
    type Error = RoughneckError;
    type Args = EditFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Apply a string patch to a file.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "search": {"type": "string"},
                    "replace": {"type": "string"},
                    "replace_all": {"type": "boolean"}
                },
                "required": ["path", "search", "replace"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        self.fs
            .edit_file(
                &args.path,
                FilePatch {
                    search: args.search,
                    replace: args.replace,
                    replace_all: args.replace_all,
                },
            )
            .await?;
        Ok(json!({"ok": true, "path": args.path}))
    }
}

#[derive(Debug)]
pub struct GlobTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobArgs {
    pub pattern: String,
}

impl GlobTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for GlobTool {
    const NAME: &'static str = "glob";
    type Error = RoughneckError;
    type Args = GlobArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find files matching a glob pattern.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"pattern": {"type": "string"}},
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let matches = self.fs.glob(&args.pattern).await?;
        Ok(json!({"matches": matches}))
    }
}

#[derive(Debug)]
pub struct GrepTool {
    fs: Arc<dyn FileSystemBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GrepArgs {
    pub pattern: String,
    #[serde(default)]
    pub paths: Vec<String>,
}

impl GrepTool {
    pub fn new(fs: Arc<dyn FileSystemBackend>) -> Self {
        Self { fs }
    }
}

impl Tool for GrepTool {
    const NAME: &'static str = "grep";
    type Error = RoughneckError;
    type Args = GrepArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search file contents with a regex pattern.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "paths": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let matches = self.fs.grep(&args.pattern, args.paths).await?;
        Ok(json!({"matches": matches}))
    }
}

#[derive(Debug)]
pub struct ExecuteTool {
    fs: Arc<dyn FileSystemBackend>,
    default_timeout_secs: u64,
    max_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExecuteArgs {
    pub cmd: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl ExecuteTool {
    pub fn new(
        fs: Arc<dyn FileSystemBackend>,
        default_timeout_secs: u64,
        max_timeout_secs: u64,
    ) -> Self {
        Self {
            fs,
            default_timeout_secs,
            max_timeout_secs,
        }
    }
}

impl Tool for ExecuteTool {
    const NAME: &'static str = "execute";
    type Error = RoughneckError;
    type Args = ExecuteArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a shell command in the configured sandbox.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string"},
                    "timeout_secs": {"type": "integer", "minimum": 1}
                },
                "required": ["cmd"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let timeout_secs = args
            .timeout_secs
            .unwrap_or(self.default_timeout_secs)
            .min(self.max_timeout_secs)
            .max(1);
        let result = self
            .fs
            .execute(&args.cmd, Duration::from_secs(timeout_secs))
            .await?;
        Ok(json!({"result": result}))
    }
}

pub fn builtin_tools(
    fs: Arc<dyn FileSystemBackend>,
    default_timeout_secs: u64,
    max_timeout_secs: u64,
) -> Vec<Box<dyn ToolDyn>> {
    vec![
        Box::new(LsTool::new(fs.clone())),
        Box::new(ReadFileTool::new(fs.clone())),
        Box::new(WriteFileTool::new(fs.clone())),
        Box::new(EditFileTool::new(fs.clone())),
        Box::new(GlobTool::new(fs.clone())),
        Box::new(GrepTool::new(fs.clone())),
        Box::new(ExecuteTool::new(fs, default_timeout_secs, max_timeout_secs)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryFileSystemBackend;

    #[tokio::test]
    async fn tools_can_read_and_write() {
        let fs: Arc<dyn FileSystemBackend> = Arc::new(InMemoryFileSystemBackend::default());
        let write = WriteFileTool::new(fs.clone());
        rig::tool::Tool::call(
            &write,
            WriteFileArgs {
                path: "notes.txt".to_string(),
                content: "hello".to_string(),
            },
        )
        .await
        .unwrap();

        let read = ReadFileTool::new(fs);
        let output = rig::tool::Tool::call(
            &read,
            ReadFileArgs {
                path: "notes.txt".to_string(),
                start: None,
                end: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(output["content"], "hello");
    }
}
