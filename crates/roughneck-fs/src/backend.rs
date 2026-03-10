use async_trait::async_trait;
use globset::{Glob, GlobSetBuilder};
use regex::Regex;
use roughneck_core::{
    ExecutionResult, FileInfo, FilePatch, FileSystemBackend, GrepMatch, LineRange, Result,
    RoughneckError,
};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;

fn normalize_rel_path(path: &str) -> Result<String> {
    let trimmed = path.trim();
    let input = if trimmed.is_empty() { "." } else { trimmed };
    let mut normalized = PathBuf::new();
    for component in Path::new(input).components() {
        match component {
            Component::ParentDir => {
                return Err(RoughneckError::InvalidInput(format!(
                    "path traversal is not allowed: {input}"
                )));
            }
            Component::Normal(part) => normalized.push(part),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

fn apply_line_range(content: &str, range: Option<LineRange>) -> String {
    if let Some(range) = range {
        if range.start == 0 || range.end < range.start {
            return String::new();
        }
        let lines: Vec<&str> = content.lines().collect();
        let start = range.start.saturating_sub(1).min(lines.len());
        let end = range.end.min(lines.len());
        return lines[start..end].join("\n");
    }
    content.to_string()
}

#[derive(Debug)]
pub struct InMemoryFileSystemBackend {
    files: tokio::sync::RwLock<HashMap<String, String>>,
    execute_enabled: bool,
}

impl InMemoryFileSystemBackend {
    #[must_use]
    pub fn new(execute_enabled: bool) -> Self {
        Self {
            files: tokio::sync::RwLock::new(HashMap::new()),
            execute_enabled,
        }
    }
}

impl Default for InMemoryFileSystemBackend {
    fn default() -> Self {
        Self::new(false)
    }
}

#[async_trait]
impl FileSystemBackend for InMemoryFileSystemBackend {
    async fn ls(&self, path: &str) -> Result<Vec<FileInfo>> {
        let normalized = normalize_rel_path(path)?;
        let prefix = if normalized.is_empty() {
            String::new()
        } else {
            format!("{normalized}/")
        };

        let guard = self.files.read().await;
        let mut entries: BTreeMap<String, FileInfo> = BTreeMap::new();

        for (file_path, content) in &*guard {
            if !(prefix.is_empty() || file_path == &normalized || file_path.starts_with(&prefix)) {
                continue;
            }

            let rest = if prefix.is_empty() {
                file_path.as_str()
            } else if file_path == &normalized {
                ""
            } else {
                file_path.trim_start_matches(&prefix)
            };

            if rest.is_empty() {
                entries.insert(
                    normalized.clone(),
                    FileInfo {
                        path: normalized.clone(),
                        is_dir: false,
                        size: content.len() as u64,
                    },
                );
                continue;
            }

            let segment = rest.split('/').next().unwrap_or_default();
            let entry_path = if normalized.is_empty() {
                segment.to_string()
            } else {
                format!("{normalized}/{segment}")
            };
            let is_dir = rest.contains('/');

            entries
                .entry(entry_path.clone())
                .and_modify(|entry| {
                    if is_dir {
                        entry.is_dir = true;
                        entry.size = 0;
                    }
                })
                .or_insert(FileInfo {
                    path: entry_path,
                    is_dir,
                    size: if is_dir { 0 } else { content.len() as u64 },
                });
        }

        Ok(entries.into_values().collect())
    }

    async fn read_file(&self, path: &str, range: Option<LineRange>) -> Result<String> {
        let normalized = normalize_rel_path(path)?;
        let guard = self.files.read().await;
        let content = guard
            .get(&normalized)
            .ok_or_else(|| RoughneckError::NotFound(format!("missing file: {normalized}")))?;
        Ok(apply_line_range(content, range))
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let normalized = normalize_rel_path(path)?;
        if normalized.is_empty() {
            return Err(RoughneckError::InvalidInput(
                "cannot write to root path".to_string(),
            ));
        }
        self.files
            .write()
            .await
            .insert(normalized, content.to_string());
        Ok(())
    }

    async fn edit_file(&self, path: &str, file_patch: FilePatch) -> Result<()> {
        let normalized = normalize_rel_path(path)?;
        let mut guard = self.files.write().await;
        let content = guard
            .get_mut(&normalized)
            .ok_or_else(|| RoughneckError::NotFound(format!("missing file: {normalized}")))?;

        if file_patch.replace_all {
            *content = content.replace(&file_patch.search, &file_patch.replace);
        } else if let Some((idx, _)) = content.match_indices(&file_patch.search).next() {
            content.replace_range(idx..idx + file_patch.search.len(), &file_patch.replace);
        } else {
            return Err(RoughneckError::InvalidInput(format!(
                "pattern not found in {normalized}"
            )));
        }

        Ok(())
    }

    async fn glob(&self, pattern: &str) -> Result<Vec<FileInfo>> {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new(pattern).map_err(|e| RoughneckError::InvalidInput(e.to_string()))?);
        let matcher = builder
            .build()
            .map_err(|e| RoughneckError::InvalidInput(e.to_string()))?;

        let guard = self.files.read().await;
        let mut matched_files = Vec::new();
        for (path, content) in &*guard {
            if matcher.is_match(path) {
                matched_files.push(FileInfo {
                    path: path.clone(),
                    is_dir: false,
                    size: content.len() as u64,
                });
            }
        }
        Ok(matched_files)
    }

    async fn grep(&self, pattern: &str, paths: Vec<String>) -> Result<Vec<GrepMatch>> {
        let regex = Regex::new(pattern)
            .map_err(|e| RoughneckError::InvalidInput(format!("invalid regex: {e}")))?;
        let include_paths: Vec<String> = paths
            .into_iter()
            .map(|p| normalize_rel_path(&p))
            .collect::<Result<Vec<_>>>()?;

        let guard = self.files.read().await;
        let mut out = Vec::new();
        for (path, content) in &*guard {
            if !include_paths.is_empty()
                && !include_paths.iter().any(|candidate| {
                    path == candidate || path.starts_with(&format!("{candidate}/"))
                })
            {
                continue;
            }

            for (idx, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    out.push(GrepMatch {
                        path: path.clone(),
                        line_number: idx + 1,
                        line: line.to_string(),
                    });
                }
            }
        }
        Ok(out)
    }

    async fn execute(&self, cmd: &str, timeout: Duration) -> Result<ExecutionResult> {
        if !self.execute_enabled {
            return Err(RoughneckError::Unsupported(
                "execute tool is disabled".to_string(),
            ));
        }

        let run = Command::new("sh").arg("-lc").arg(cmd).output();
        match tokio::time::timeout(timeout, run).await {
            Ok(output) => {
                let output = output?;
                Ok(ExecutionResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or_default(),
                    timed_out: false,
                })
            }
            Err(_) => Ok(ExecutionResult {
                stdout: String::new(),
                stderr: format!("command timed out after {}s", timeout.as_secs()),
                exit_code: -1,
                timed_out: true,
            }),
        }
    }

    async fn snapshot(&self) -> Result<HashMap<String, String>> {
        Ok(self.files.read().await.clone())
    }
}

#[derive(Debug)]
pub struct LocalFsBackend {
    root: PathBuf,
    execute_enabled: bool,
}

impl LocalFsBackend {
    #[must_use]
    pub fn new(root: PathBuf, execute_enabled: bool) -> Self {
        Self {
            root,
            execute_enabled,
        }
    }

    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let normalized = normalize_rel_path(path)?;
        Ok(if normalized.is_empty() {
            self.root.clone()
        } else {
            self.root.join(normalized)
        })
    }

    fn rel(&self, path: &Path) -> Result<String> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| RoughneckError::InvalidInput("path escaped root".to_string()))?;
        Ok(rel.to_string_lossy().replace('\\', "/"))
    }
}

#[async_trait]
impl FileSystemBackend for LocalFsBackend {
    async fn ls(&self, path: &str) -> Result<Vec<FileInfo>> {
        let target = self.resolve(path)?;
        let mut out = Vec::new();
        let mut dir = fs::read_dir(target).await?;
        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            out.push(FileInfo {
                path: self.rel(&entry.path())?,
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn read_file(&self, path: &str, range: Option<LineRange>) -> Result<String> {
        let content = fs::read_to_string(self.resolve(path)?).await?;
        Ok(apply_line_range(&content, range))
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(full, content).await?;
        Ok(())
    }

    async fn edit_file(&self, path: &str, file_patch: FilePatch) -> Result<()> {
        let full = self.resolve(path)?;
        let mut content = fs::read_to_string(&full).await?;
        if file_patch.replace_all {
            content = content.replace(&file_patch.search, &file_patch.replace);
        } else if let Some((idx, _)) = content.match_indices(&file_patch.search).next() {
            content.replace_range(idx..idx + file_patch.search.len(), &file_patch.replace);
        } else {
            return Err(RoughneckError::InvalidInput(format!(
                "pattern not found: {}",
                file_patch.search
            )));
        }
        fs::write(full, content).await?;
        Ok(())
    }

    async fn glob(&self, pattern: &str) -> Result<Vec<FileInfo>> {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new(pattern).map_err(|e| RoughneckError::InvalidInput(e.to_string()))?);
        let matcher = builder
            .build()
            .map_err(|e| RoughneckError::InvalidInput(e.to_string()))?;

        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let rel = self.rel(entry.path())?;
            if matcher.is_match(&rel) {
                out.push(FileInfo {
                    path: rel,
                    is_dir: false,
                    size: entry.metadata().map(|m| m.len()).unwrap_or_default(),
                });
            }
        }
        Ok(out)
    }

    async fn grep(&self, pattern: &str, paths: Vec<String>) -> Result<Vec<GrepMatch>> {
        let regex = Regex::new(pattern)
            .map_err(|e| RoughneckError::InvalidInput(format!("invalid regex: {e}")))?;
        let include_paths: Vec<String> = paths
            .into_iter()
            .map(|p| normalize_rel_path(&p))
            .collect::<Result<Vec<_>>>()?;

        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let rel = self.rel(entry.path())?;
            if !include_paths.is_empty()
                && !include_paths
                    .iter()
                    .any(|candidate| rel == *candidate || rel.starts_with(&format!("{candidate}/")))
            {
                continue;
            }

            if let Ok(content) = fs::read_to_string(entry.path()).await {
                for (idx, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        out.push(GrepMatch {
                            path: rel.clone(),
                            line_number: idx + 1,
                            line: line.to_string(),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    async fn execute(&self, cmd: &str, timeout: Duration) -> Result<ExecutionResult> {
        if !self.execute_enabled {
            return Err(RoughneckError::Unsupported(
                "execute tool is disabled".to_string(),
            ));
        }

        let run = Command::new("sh")
            .arg("-lc")
            .arg(cmd)
            .current_dir(&self.root)
            .output();

        match tokio::time::timeout(timeout, run).await {
            Ok(output) => {
                let output = output?;
                Ok(ExecutionResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or_default(),
                    timed_out: false,
                })
            }
            Err(_) => Ok(ExecutionResult {
                stdout: String::new(),
                stderr: format!("command timed out after {}s", timeout.as_secs()),
                exit_code: -1,
                timed_out: true,
            }),
        }
    }

    async fn snapshot(&self) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            if let Ok(content) = fs::read_to_string(entry.path()).await {
                files.insert(self.rel(entry.path())?, content);
            }
        }
        Ok(files)
    }
}
