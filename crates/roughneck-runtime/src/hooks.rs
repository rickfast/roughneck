use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolDyn, ToolError};
use roughneck_core::{
    HookOutputSummary, HookRule, HooksConfig, MemoryBackend, MemoryEvent, MemoryScope, Result,
    RoughneckError, now_millis,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Notification,
    Stop,
    SubagentStop,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::Notification => "Notification",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
        }
    }

    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "pre_tool_use" | "preToolUse" | "PreToolUse" => Some(Self::PreToolUse),
            "post_tool_use" | "postToolUse" | "PostToolUse" => Some(Self::PostToolUse),
            "notification" | "Notification" => Some(Self::Notification),
            "stop" | "Stop" => Some(Self::Stop),
            "subagent_stop" | "subagentStop" | "SubagentStop" => Some(Self::SubagentStop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HookContext {
    pub session_id: String,
    pub invocation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl HookContext {
    #[must_use]
    pub fn new(
        session_id: impl Into<String>,
        invocation_id: impl Into<String>,
        tool_call_id: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            invocation_id: invocation_id.into(),
            tool_call_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub hook_event_name: String,
    pub cwd: String,
    pub session_id: String,
    pub invocation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HookPayload {
    fn from_context(event: HookEvent, cwd: &str, ctx: &HookContext) -> Self {
        Self {
            hook_event_name: event.as_str().to_string(),
            cwd: cwd.to_string(),
            session_id: ctx.session_id.clone(),
            invocation_id: ctx.invocation_id.clone(),
            tool_call_id: ctx.tool_call_id.clone(),
            tool_name: None,
            tool_input: None,
            tool_response: None,
            tool_error: None,
            message: None,
            reason: None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct HookCommandOutput {
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    messages: Vec<String>,
    #[serde(default)]
    suppress_output: Option<bool>,
    #[serde(default)]
    hook_specific_output: Option<Value>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HookDecision {
    pub blocked: bool,
    pub reason: Option<String>,
    pub suppress_output: bool,
    pub hook_specific_output: Vec<Value>,
    pub messages: Vec<String>,
}

impl HookDecision {
    pub fn merge(&mut self, mut other: Self) {
        self.suppress_output |= other.suppress_output;
        self.messages.append(&mut other.messages);
        self.hook_specific_output
            .append(&mut other.hook_specific_output);

        if self.reason.is_none() {
            self.reason = other.reason.clone();
        }

        if other.blocked {
            self.blocked = true;
            if self.reason.is_none() {
                self.reason = other.reason.take();
            }
        }
    }
}

#[async_trait]
pub trait HookExecutor: Send + Sync + std::fmt::Debug {
    fn has_handlers(&self) -> bool;

    async fn execute(&self, event: HookEvent, payload: HookPayload) -> Result<HookDecision>;
}

#[derive(Debug, Default)]
pub struct HookCapture {
    summary: tokio::sync::Mutex<HookOutputSummary>,
}

impl HookCapture {
    pub async fn record(&self, decision: &HookDecision) {
        let mut summary = self.summary.lock().await;
        summary.messages.extend(decision.messages.iter().cloned());
        summary
            .outputs
            .extend(decision.hook_specific_output.iter().cloned());
    }

    pub async fn record_suppressed_tool(&self, tool_name: &str) {
        let mut summary = self.summary.lock().await;
        summary.suppressed_tools.push(tool_name.to_string());
    }

    pub async fn snapshot(&self) -> HookOutputSummary {
        self.summary.lock().await.clone()
    }
}

#[derive(Debug, Clone)]
#[must_use]
pub struct HookManager {
    config: HooksConfig,
    cwd: String,
    executor: Option<Arc<dyn HookExecutor>>,
}

impl HookManager {
    /// Creates a hook manager for command-based hooks only.
    ///
    /// # Errors
    ///
    /// Returns an error if the current working directory cannot be resolved.
    pub fn new(config: HooksConfig) -> Result<Self> {
        Self::new_with_executor(config, None)
    }

    /// Creates a hook manager with an optional in-process hook executor.
    ///
    /// # Errors
    ///
    /// Returns an error if the current working directory cannot be resolved.
    pub fn new_with_executor(
        config: HooksConfig,
        executor: Option<Arc<dyn HookExecutor>>,
    ) -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|err| RoughneckError::Runtime(err.to_string()))?
            .to_string_lossy()
            .to_string();
        Ok(Self {
            config,
            cwd,
            executor,
        })
    }

    pub fn with_executor(&self, executor: Arc<dyn HookExecutor>) -> Self {
        Self {
            config: self.config.clone(),
            cwd: self.cwd.clone(),
            executor: Some(executor),
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.config.enabled
            || self
                .executor
                .as_ref()
                .is_some_and(|executor| executor.has_handlers())
    }

    /// Runs the `PreToolUse` hook chain for a tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if hook serialization or hook execution fails.
    pub async fn pre_tool_use(
        &self,
        ctx: &HookContext,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookDecision> {
        let mut payload = HookPayload::from_context(HookEvent::PreToolUse, &self.cwd, ctx);
        payload.tool_name = Some(tool_name.to_string());
        payload.tool_input = Some(tool_input.clone());
        self.run(HookEvent::PreToolUse, Some(tool_name), payload)
            .await
    }

    /// Runs the `PostToolUse` hook chain for a tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if hook serialization or hook execution fails.
    pub async fn post_tool_use(
        &self,
        ctx: &HookContext,
        tool_name: &str,
        tool_input: &Value,
        tool_response: Option<&Value>,
        tool_error: Option<&str>,
    ) -> Result<HookDecision> {
        let mut payload = HookPayload::from_context(HookEvent::PostToolUse, &self.cwd, ctx);
        payload.tool_name = Some(tool_name.to_string());
        payload.tool_input = Some(tool_input.clone());
        payload.tool_response = tool_response.cloned();
        payload.tool_error = tool_error.map(str::to_string);
        self.run(HookEvent::PostToolUse, Some(tool_name), payload)
            .await
    }

    /// Runs the `Notification` hook chain.
    ///
    /// # Errors
    ///
    /// Returns an error if hook serialization or hook execution fails.
    pub async fn notification(
        &self,
        ctx: &HookContext,
        message: &str,
        payload: Option<&Value>,
    ) -> Result<HookDecision> {
        let mut hook_payload = HookPayload::from_context(HookEvent::Notification, &self.cwd, ctx);
        hook_payload.tool_input = payload.cloned();
        hook_payload.message = Some(message.to_string());
        self.run(HookEvent::Notification, None, hook_payload).await
    }

    /// Runs the `Stop` hook chain.
    ///
    /// # Errors
    ///
    /// Returns an error if hook serialization or hook execution fails.
    pub async fn stop(
        &self,
        ctx: &HookContext,
        reason: &str,
        payload: Option<&Value>,
    ) -> Result<HookDecision> {
        let mut hook_payload = HookPayload::from_context(HookEvent::Stop, &self.cwd, ctx);
        hook_payload.tool_input = payload.cloned();
        hook_payload.reason = Some(reason.to_string());
        self.run(HookEvent::Stop, None, hook_payload).await
    }

    /// Runs the `SubagentStop` hook chain.
    ///
    /// # Errors
    ///
    /// Returns an error if hook serialization or hook execution fails.
    pub async fn subagent_stop(
        &self,
        ctx: &HookContext,
        reason: &str,
        payload: Option<&Value>,
    ) -> Result<HookDecision> {
        let mut hook_payload = HookPayload::from_context(HookEvent::SubagentStop, &self.cwd, ctx);
        hook_payload.tool_input = payload.cloned();
        hook_payload.reason = Some(reason.to_string());
        self.run(HookEvent::SubagentStop, None, hook_payload).await
    }

    async fn run(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        payload: HookPayload,
    ) -> Result<HookDecision> {
        if !self.is_active() {
            return Ok(HookDecision::default());
        }
        let mut aggregate = HookDecision::default();

        if self.config.enabled {
            let rules = self.rules_for_event(event);
            let payload_bytes = serde_json::to_vec(&payload)?;
            for rule in rules {
                if !matches_rule(rule, tool_name)? {
                    continue;
                }

                let timeout_secs = rule.timeout_secs.unwrap_or(self.config.timeout_secs).max(1);
                let result = self
                    .run_command(
                        &rule.command,
                        &payload_bytes,
                        Duration::from_secs(timeout_secs),
                    )
                    .await?;
                aggregate.merge(result);
                if aggregate.blocked {
                    return Ok(aggregate);
                }
            }
        }

        if let Some(executor) = &self.executor {
            if !executor.has_handlers() {
                return Ok(aggregate);
            }
            let result = executor.execute(event, payload).await?;
            aggregate.merge(result);
        }

        Ok(aggregate)
    }

    fn rules_for_event(&self, event: HookEvent) -> &[HookRule] {
        match event {
            HookEvent::PreToolUse => &self.config.pre_tool_use,
            HookEvent::PostToolUse => &self.config.post_tool_use,
            HookEvent::Notification => &self.config.notification,
            HookEvent::Stop => &self.config.stop,
            HookEvent::SubagentStop => &self.config.subagent_stop,
        }
    }

    async fn run_command(
        &self,
        command: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<HookDecision> {
        let mut child = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| RoughneckError::Runtime(err.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(payload)
                .await
                .map_err(|err| RoughneckError::Runtime(err.to_string()))?;
        }

        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| RoughneckError::Runtime("hook command timed out".to_string()))?
            .map_err(|err| RoughneckError::Runtime(err.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let parsed = if stdout.is_empty() {
            HookCommandOutput::default()
        } else {
            serde_json::from_str::<HookCommandOutput>(&stdout).unwrap_or_else(|_| {
                HookCommandOutput {
                    messages: vec![stdout.clone()],
                    ..HookCommandOutput::default()
                }
            })
        };

        let mut decision = HookDecision {
            blocked: false,
            reason: parsed.reason.clone(),
            suppress_output: parsed.suppress_output.unwrap_or(false),
            hook_specific_output: parsed
                .hook_specific_output
                .into_iter()
                .collect::<Vec<Value>>(),
            messages: parsed.messages,
        };

        match output.status.code() {
            Some(0) => {
                if let Some(mode) = parsed.decision
                    && mode.eq_ignore_ascii_case("block")
                {
                    decision.blocked = true;
                    if decision.reason.is_none() {
                        decision.reason = Some("blocked by hook".to_string());
                    }
                }
            }
            Some(2) => {
                decision.blocked = true;
                if decision.reason.is_none() {
                    decision.reason = Some(if stdout.is_empty() {
                        "blocked by hook (exit code 2)".to_string()
                    } else {
                        stdout
                    });
                }
            }
            _ => {
                if !stderr.is_empty() {
                    decision
                        .messages
                        .push(format!("hook command error: {stderr}"));
                }
            }
        }

        Ok(decision)
    }
}

fn matches_rule(rule: &HookRule, tool_name: Option<&str>) -> Result<bool> {
    let Some(tool_name) = tool_name else {
        return Ok(true);
    };

    let pattern = if rule.matcher.is_empty() {
        "*"
    } else {
        rule.matcher.as_str()
    };

    let matcher: GlobMatcher = Glob::new(pattern)
        .map_err(|err| RoughneckError::Config(format!("invalid hook matcher '{pattern}': {err}")))?
        .compile_matcher();

    Ok(matcher.is_match(tool_name))
}

#[derive(Debug)]
pub struct ToolRuntimeContext {
    pub session_id: String,
    pub invocation_id: String,
    pub memory: Arc<dyn MemoryBackend>,
    pub hook_capture: Arc<HookCapture>,
    pub tool_call_counter: Arc<AtomicUsize>,
}

impl ToolRuntimeContext {
    fn next_tool_call_id(&self) -> String {
        let idx = self.tool_call_counter.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{}-tool-{idx}", self.invocation_id)
    }

    async fn append_tool_event(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        args: &Value,
        response: Option<&Value>,
        error: Option<&str>,
    ) -> Result<()> {
        self.memory
            .append_event(
                &self.session_id,
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "tool_call".to_string(),
                    payload: json!({
                        "tool": tool_name,
                        "tool_call_id": tool_call_id,
                        "args": args,
                        "response": response,
                        "error": error,
                    }),
                    timestamp_ms: now_millis(),
                },
            )
            .await
    }
}

#[derive(Debug)]
pub struct HookedTool<T>
where
    T: Tool<Error = RoughneckError, Output = Value>,
{
    inner: T,
    hooks: Arc<HookManager>,
    runtime: Arc<ToolRuntimeContext>,
}

impl<T> HookedTool<T>
where
    T: Tool<Error = RoughneckError, Output = Value>,
{
    pub fn new(inner: T, hooks: Arc<HookManager>, runtime: Arc<ToolRuntimeContext>) -> Self {
        Self {
            inner,
            hooks,
            runtime,
        }
    }
}

#[derive(Clone)]
pub struct HookedToolDyn {
    inner: Arc<dyn ToolDyn>,
    hooks: Arc<HookManager>,
    runtime: Arc<ToolRuntimeContext>,
}

impl HookedToolDyn {
    #[must_use]
    pub fn new(
        inner: Arc<dyn ToolDyn>,
        hooks: Arc<HookManager>,
        runtime: Arc<ToolRuntimeContext>,
    ) -> Self {
        Self {
            inner,
            hooks,
            runtime,
        }
    }
}

impl std::fmt::Debug for HookedToolDyn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookedToolDyn")
            .field("name", &self.inner.name())
            .finish()
    }
}

impl ToolDyn for HookedToolDyn {
    fn name(&self) -> String {
        self.inner.name()
    }

    fn definition<'a>(
        &'a self,
        prompt: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, ToolDefinition> {
        self.inner.definition(prompt)
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<String, ToolError>> {
        Box::pin(async move {
            let args_value = serde_json::from_str::<Value>(&args)
                .unwrap_or_else(|_| Value::String(args.clone()));
            let tool_call_id = self.runtime.next_tool_call_id();
            let hook_ctx = HookContext::new(
                self.runtime.session_id.clone(),
                self.runtime.invocation_id.clone(),
                Some(tool_call_id.clone()),
            );
            let tool_name = self.inner.name();

            let pre = self
                .hooks
                .pre_tool_use(&hook_ctx, &tool_name, &args_value)
                .await
                .map_err(tool_error_from_runtime)?;
            self.runtime.hook_capture.record(&pre).await;
            if pre.blocked {
                return Err(tool_error_from_message(pre.reason.unwrap_or_else(|| {
                    format!("{tool_name} blocked by pre-tool hook")
                })));
            }

            match self.inner.call(args).await {
                Ok(output) => {
                    let output_value = serde_json::from_str::<Value>(&output)
                        .unwrap_or_else(|_| Value::String(output.clone()));
                    self.runtime
                        .append_tool_event(
                            &tool_name,
                            &tool_call_id,
                            &args_value,
                            Some(&output_value),
                            None,
                        )
                        .await
                        .map_err(tool_error_from_runtime)?;

                    let post = self
                        .hooks
                        .post_tool_use(
                            &hook_ctx,
                            &tool_name,
                            &args_value,
                            Some(&output_value),
                            None,
                        )
                        .await
                        .map_err(tool_error_from_runtime)?;
                    self.runtime.hook_capture.record(&post).await;
                    if post.blocked {
                        return Err(tool_error_from_message(post.reason.unwrap_or_else(|| {
                            format!("{tool_name} blocked by post-tool hook")
                        })));
                    }
                    if post.suppress_output {
                        self.runtime
                            .hook_capture
                            .record_suppressed_tool(&tool_name)
                            .await;
                        return serde_json::to_string(&json!({
                            "suppressed": true,
                            "tool": tool_name,
                            "message": "tool output suppressed by hook",
                        }))
                        .map_err(ToolError::JsonError);
                    }

                    Ok(output)
                }
                Err(err) => {
                    let error_string = err.to_string();
                    let _ = self
                        .runtime
                        .append_tool_event(
                            &tool_name,
                            &tool_call_id,
                            &args_value,
                            None,
                            Some(&error_string),
                        )
                        .await;
                    let post = self
                        .hooks
                        .post_tool_use(
                            &hook_ctx,
                            &tool_name,
                            &args_value,
                            None,
                            Some(&error_string),
                        )
                        .await
                        .map_err(tool_error_from_runtime)?;
                    self.runtime.hook_capture.record(&post).await;
                    if post.blocked {
                        return Err(tool_error_from_message(post.reason.unwrap_or_else(|| {
                            format!("{tool_name} blocked by post-tool hook")
                        })));
                    }
                    Err(err)
                }
            }
        })
    }
}

fn tool_error_from_runtime(error: RoughneckError) -> ToolError {
    ToolError::ToolCallError(Box::new(error))
}

fn tool_error_from_message(message: String) -> ToolError {
    ToolError::ToolCallError(Box::new(RoughneckError::Runtime(message)))
}

impl<T> Tool for HookedTool<T>
where
    T: Tool<Error = RoughneckError, Output = Value> + Send + Sync,
    T::Args: Serialize + Send + Sync,
{
    const NAME: &'static str = T::NAME;
    type Error = RoughneckError;
    type Args = T::Args;
    type Output = Value;

    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner.definition(prompt).await
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let args_value = serde_json::to_value(&args).unwrap_or(Value::Null);
        let tool_call_id = self.runtime.next_tool_call_id();
        let hook_ctx = HookContext::new(
            self.runtime.session_id.clone(),
            self.runtime.invocation_id.clone(),
            Some(tool_call_id.clone()),
        );

        let pre = self
            .hooks
            .pre_tool_use(&hook_ctx, T::NAME, &args_value)
            .await?;
        self.runtime.hook_capture.record(&pre).await;
        if pre.blocked {
            return Err(RoughneckError::Runtime(pre.reason.unwrap_or_else(|| {
                format!("{} blocked by pre-tool hook", T::NAME)
            })));
        }

        let result = self.inner.call(args).await;
        match result {
            Ok(output) => {
                self.runtime
                    .append_tool_event(T::NAME, &tool_call_id, &args_value, Some(&output), None)
                    .await?;

                let post = self
                    .hooks
                    .post_tool_use(&hook_ctx, T::NAME, &args_value, Some(&output), None)
                    .await?;
                self.runtime.hook_capture.record(&post).await;
                if post.blocked {
                    return Err(RoughneckError::Runtime(post.reason.unwrap_or_else(|| {
                        format!("{} blocked by post-tool hook", T::NAME)
                    })));
                }
                if post.suppress_output {
                    self.runtime
                        .hook_capture
                        .record_suppressed_tool(T::NAME)
                        .await;
                    return Ok(json!({
                        "suppressed": true,
                        "tool": T::NAME,
                        "message": "tool output suppressed by hook",
                    }));
                }
                Ok(output)
            }
            Err(err) => {
                let _ = self
                    .runtime
                    .append_tool_event(
                        T::NAME,
                        &tool_call_id,
                        &args_value,
                        None,
                        Some(&err.to_string()),
                    )
                    .await;
                let post = self
                    .hooks
                    .post_tool_use(
                        &hook_ctx,
                        T::NAME,
                        &args_value,
                        None,
                        Some(&err.to_string()),
                    )
                    .await?;
                self.runtime.hook_capture.record(&post).await;
                if post.blocked {
                    return Err(RoughneckError::Runtime(post.reason.unwrap_or_else(|| {
                        format!("{} blocked by post-tool hook", T::NAME)
                    })));
                }
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roughneck_memory::InMemoryMemoryBackend;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubExecutor {
        calls: Mutex<Vec<HookPayload>>,
        decisions: HashMap<HookEvent, HookDecision>,
    }

    impl std::fmt::Debug for StubExecutor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StubExecutor").finish()
        }
    }

    #[async_trait]
    impl HookExecutor for StubExecutor {
        fn has_handlers(&self) -> bool {
            !self.decisions.is_empty()
        }

        async fn execute(&self, event: HookEvent, payload: HookPayload) -> Result<HookDecision> {
            self.calls.lock().unwrap().push(payload);
            Ok(self.decisions.get(&event).cloned().unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn disabled_hooks_are_noop() {
        let manager = HookManager::new(HooksConfig::default()).unwrap();
        let decision = manager
            .notification(
                &HookContext::new("session", "invoke", None),
                "hello",
                Some(&Value::String("payload".to_string())),
            )
            .await
            .unwrap();
        assert!(!decision.blocked);
    }

    #[tokio::test]
    async fn executor_runs_even_when_shell_hooks_are_disabled() {
        let executor = Arc::new(StubExecutor {
            decisions: HashMap::from([(
                HookEvent::Notification,
                HookDecision {
                    messages: vec!["from executor".to_string()],
                    suppress_output: true,
                    ..HookDecision::default()
                },
            )]),
            ..StubExecutor::default()
        });
        let manager =
            HookManager::new_with_executor(HooksConfig::default(), Some(executor.clone())).unwrap();

        let decision = manager
            .notification(
                &HookContext::new("session-123", "invoke-456", None),
                "hello",
                Some(&json!({"state": "ok"})),
            )
            .await
            .unwrap();

        assert_eq!(decision.messages, vec!["from executor".to_string()]);
        assert!(decision.suppress_output);

        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].session_id, "session-123");
        assert_eq!(calls[0].hook_event_name, "Notification");
    }

    #[derive(Debug)]
    struct EchoTool;

    #[derive(Debug, Serialize, Deserialize)]
    struct EchoArgs {
        value: String,
    }

    impl Tool for EchoTool {
        const NAME: &'static str = "echo";
        type Error = RoughneckError;
        type Args = EchoArgs;
        type Output = Value;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "echo".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "value": {"type": "string"}
                    },
                    "required": ["value"]
                }),
            }
        }

        async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
            Ok(json!({"echo": args.value}))
        }
    }

    #[derive(Debug)]
    struct DynamicEchoTool;

    impl ToolDyn for DynamicEchoTool {
        fn name(&self) -> String {
            "dynamic_echo".to_string()
        }

        fn definition<'a>(
            &'a self,
            _prompt: String,
        ) -> rig::wasm_compat::WasmBoxedFuture<'a, ToolDefinition> {
            Box::pin(async move {
                ToolDefinition {
                    name: self.name(),
                    description: "echo".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "value": {"type": "string"}
                        },
                        "required": ["value"]
                    }),
                }
            })
        }

        fn call<'a>(
            &'a self,
            args: String,
        ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<String, ToolError>> {
            Box::pin(async move {
                let args: Value = serde_json::from_str(&args).map_err(ToolError::JsonError)?;
                let value = args
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                serde_json::to_string(&json!({"echo": value})).map_err(ToolError::JsonError)
            })
        }
    }

    #[tokio::test]
    async fn post_tool_hook_can_suppress_output_and_record_metadata() {
        let hooks = HookManager::new(HooksConfig {
            enabled: true,
            timeout_secs: 5,
            post_tool_use: vec![HookRule {
                matcher: "echo".to_string(),
                command: "printf '{\"suppress_output\":true,\"messages\":[\"masked\"],\"hook_specific_output\":{\"policy\":\"redact\"}}'".to_string(),
                timeout_secs: None,
            }],
            ..HooksConfig::default()
        })
        .unwrap();
        let memory = Arc::new(InMemoryMemoryBackend::default());
        let capture = Arc::new(HookCapture::default());
        let runtime = Arc::new(ToolRuntimeContext {
            session_id: "session".to_string(),
            invocation_id: "invoke".to_string(),
            memory: memory.clone(),
            hook_capture: capture.clone(),
            tool_call_counter: Arc::new(AtomicUsize::new(0)),
        });

        let tool = HookedTool::new(EchoTool, Arc::new(hooks), runtime);
        let result = Tool::call(
            &tool,
            EchoArgs {
                value: "hello".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(result["suppressed"], Value::Bool(true));
        let summary = capture.snapshot().await;
        assert_eq!(summary.messages, vec!["masked".to_string()]);
        assert_eq!(summary.suppressed_tools, vec!["echo".to_string()]);
        assert_eq!(summary.outputs.len(), 1);

        let events = memory.get_events("session", usize::MAX).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "tool_call"));
    }

    #[tokio::test]
    async fn dynamic_tool_wrapper_runs_hooks_and_records_tool_calls() {
        let executor = Arc::new(StubExecutor {
            decisions: HashMap::from([(
                HookEvent::PostToolUse,
                HookDecision {
                    messages: vec!["dynamic masked".to_string()],
                    suppress_output: true,
                    ..HookDecision::default()
                },
            )]),
            ..StubExecutor::default()
        });
        let hooks =
            HookManager::new_with_executor(HooksConfig::default(), Some(executor.clone())).unwrap();
        let memory = Arc::new(InMemoryMemoryBackend::default());
        let capture = Arc::new(HookCapture::default());
        let runtime = Arc::new(ToolRuntimeContext {
            session_id: "session".to_string(),
            invocation_id: "invoke".to_string(),
            memory: memory.clone(),
            hook_capture: capture.clone(),
            tool_call_counter: Arc::new(AtomicUsize::new(0)),
        });

        let tool = HookedToolDyn::new(Arc::new(DynamicEchoTool), Arc::new(hooks), runtime);
        let result = ToolDyn::call(&tool, "{\"value\":\"hello\"}".to_string())
            .await
            .unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(result["suppressed"], Value::Bool(true));
        let summary = capture.snapshot().await;
        assert_eq!(summary.messages, vec!["dynamic masked".to_string()]);
        assert_eq!(summary.suppressed_tools, vec!["dynamic_echo".to_string()]);

        let events = memory.get_events("session", usize::MAX).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "tool_call"));

        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].hook_event_name, "PreToolUse");
        assert_eq!(calls[0].tool_name.as_deref(), Some("dynamic_echo"));
        assert_eq!(calls[1].hook_event_name, "PostToolUse");
        assert_eq!(
            calls[1].tool_call_id.as_deref(),
            calls[0].tool_call_id.as_deref()
        );
    }
}
