use crate::error::Result;
use crate::types::{ExecutionResult, FileInfo, FilePatch, GrepMatch, LineRange, MemoryEvent};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

#[async_trait]
pub trait FileSystemBackend: Send + Sync + Debug {
    async fn ls(&self, path: &str) -> Result<Vec<FileInfo>>;
    async fn read_file(&self, path: &str, range: Option<LineRange>) -> Result<String>;
    async fn write_file(&self, path: &str, content: &str) -> Result<()>;
    async fn edit_file(&self, path: &str, patch: FilePatch) -> Result<()>;
    async fn glob(&self, pattern: &str) -> Result<Vec<FileInfo>>;
    async fn grep(&self, pattern: &str, paths: Vec<String>) -> Result<Vec<GrepMatch>>;
    async fn execute(&self, cmd: &str, timeout: Duration) -> Result<ExecutionResult>;
    async fn snapshot(&self) -> Result<HashMap<String, String>>;
}

#[async_trait]
pub trait MemoryBackend: Send + Sync + Debug {
    async fn append_event(&self, conv_id: &str, event: MemoryEvent) -> Result<()>;
    async fn get_events(&self, conv_id: &str, limit: usize) -> Result<Vec<MemoryEvent>>;
    async fn search(&self, conv_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEvent>>;
}
