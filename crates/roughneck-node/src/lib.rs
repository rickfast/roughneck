use async_trait::async_trait;
use napi::bindgen_prelude::{Error, Result};
use napi::threadsafe_function::{ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction};
use napi::{JsFunction, Status};
use napi_derive::napi;
use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use roughneck_core::{
    DeepAgentConfig, Result as RoughneckResult, RoughneckError, SessionInit, SessionInvokeRequest,
};
use roughneck_runtime::{
    AgentSession as RuntimeAgentSession, DeepAgent as RuntimeDeepAgent, HookDecision, HookEvent,
    HookExecutor, HookManager, HookPayload, HookedToolDyn, ProgrammaticToolFactory,
    ToolRuntimeContext,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};

fn from_json<T>(value: Option<Value>) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    match value {
        Some(value) => {
            serde_json::from_value(value).map_err(|err| Error::from_reason(err.to_string()))
        }
        None => Ok(T::default()),
    }
}

fn parse_hook_event(value: &str) -> Result<HookEvent> {
    HookEvent::from_name(value).ok_or_else(|| {
        Error::new(
            Status::InvalidArg,
            format!(
                "unknown hook event '{value}', expected one of: preToolUse, postToolUse, notification, stop, subagentStop"
            ),
        )
    })
}

fn tool_call_error(message: impl Into<String>) -> ToolError {
    ToolError::ToolCallError(Box::new(io::Error::other(message.into())))
}

#[derive(Default)]
struct NodeHookExecutor {
    callbacks: RwLock<HashMap<HookEvent, Vec<ThreadsafeFunction<Value, ErrorStrategy::Fatal>>>>,
}

impl std::fmt::Debug for NodeHookExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let callback_count = self
            .callbacks
            .read()
            .map(|callbacks| callbacks.values().map(Vec::len).sum::<usize>())
            .unwrap_or_default();
        f.debug_struct("NodeHookExecutor")
            .field("callback_count", &callback_count)
            .finish()
    }
}

impl NodeHookExecutor {
    fn register(&self, event: HookEvent, callback: JsFunction) -> Result<()> {
        let tsfn = callback.create_threadsafe_function::<Value, Value, _, ErrorStrategy::Fatal>(
            0,
            |ctx: ThreadSafeCallContext<Value>| Ok(vec![ctx.value]),
        )?;

        let mut callbacks = self
            .callbacks
            .write()
            .map_err(|err| Error::from_reason(format!("node hook registry poisoned: {err}")))?;
        callbacks.entry(event).or_default().push(tsfn);
        Ok(())
    }

    fn callbacks_for(
        &self,
        event: HookEvent,
    ) -> RoughneckResult<Vec<ThreadsafeFunction<Value, ErrorStrategy::Fatal>>> {
        let callbacks = self.callbacks.read().map_err(|err| {
            RoughneckError::Runtime(format!("node hook registry poisoned: {err}"))
        })?;
        Ok(callbacks.get(&event).cloned().unwrap_or_default())
    }
}

#[async_trait]
impl HookExecutor for NodeHookExecutor {
    fn has_handlers(&self) -> bool {
        self.callbacks
            .read()
            .map(|callbacks| callbacks.values().any(|callbacks| !callbacks.is_empty()))
            .unwrap_or(false)
    }

    async fn execute(
        &self,
        event: HookEvent,
        payload: HookPayload,
    ) -> RoughneckResult<HookDecision> {
        let callbacks = self.callbacks_for(event)?;
        let payload_value = serde_json::to_value(&payload)
            .map_err(|err| RoughneckError::Runtime(err.to_string()))?;
        let mut aggregate = HookDecision::default();

        for callback in callbacks {
            let result = callback
                .call_async::<Value>(payload_value.clone())
                .await
                .map_err(|err| RoughneckError::Runtime(format!("node hook error: {err}")))?;

            let decision = if result.is_null() {
                HookDecision::default()
            } else {
                serde_json::from_value(result).map_err(|err| {
                    RoughneckError::Runtime(format!("node hook returned invalid payload: {err}"))
                })?
            };

            aggregate.merge(decision);
            if aggregate.blocked {
                break;
            }
        }

        Ok(aggregate)
    }
}

#[derive(Clone)]
struct NodeToolSpec {
    name: String,
    description: String,
    parameters: Value,
    callback: ThreadsafeFunction<Value, ErrorStrategy::Fatal>,
}

impl std::fmt::Debug for NodeToolSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeToolSpec")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

#[derive(Default)]
struct NodeToolRegistry {
    tools: RwLock<Vec<NodeToolSpec>>,
}

impl std::fmt::Debug for NodeToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tool_count = self
            .tools
            .read()
            .map(|tools| tools.len())
            .unwrap_or_default();
        f.debug_struct("NodeToolRegistry")
            .field("tool_count", &tool_count)
            .finish()
    }
}

impl NodeToolRegistry {
    fn register(
        &self,
        name: &str,
        description: &str,
        parameters: Value,
        callback: JsFunction,
    ) -> Result<()> {
        if name.trim().is_empty() {
            return Err(Error::new(
                Status::InvalidArg,
                "registerTool requires a non-empty tool name".to_string(),
            ));
        }
        if description.trim().is_empty() {
            return Err(Error::new(
                Status::InvalidArg,
                "registerTool requires a non-empty tool description".to_string(),
            ));
        }

        let tsfn = callback.create_threadsafe_function::<Value, Value, _, ErrorStrategy::Fatal>(
            0,
            |ctx: ThreadSafeCallContext<Value>| Ok(vec![ctx.value]),
        )?;

        let mut tools = self
            .tools
            .write()
            .map_err(|err| Error::from_reason(format!("node tool registry poisoned: {err}")))?;
        if tools.iter().any(|tool| tool.name == name) {
            return Err(Error::new(
                Status::InvalidArg,
                format!("a Node tool named '{name}' is already registered"),
            ));
        }

        tools.push(NodeToolSpec {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
            callback: tsfn,
        });
        Ok(())
    }

    fn snapshot(&self) -> RoughneckResult<Vec<NodeToolSpec>> {
        self.tools
            .read()
            .map(|tools| tools.clone())
            .map_err(|err| RoughneckError::Runtime(format!("node tool registry poisoned: {err}")))
    }
}

impl ProgrammaticToolFactory for NodeToolRegistry {
    fn build_tools(
        &self,
        hooks: Arc<HookManager>,
        runtime: Arc<ToolRuntimeContext>,
    ) -> RoughneckResult<Vec<Box<dyn ToolDyn>>> {
        let tools = self.snapshot()?;
        Ok(tools
            .into_iter()
            .map(|tool| {
                Box::new(HookedToolDyn::new(
                    Arc::new(NodeToolDyn::new(tool)),
                    hooks.clone(),
                    runtime.clone(),
                )) as Box<dyn ToolDyn>
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
struct NodeToolDyn {
    spec: NodeToolSpec,
}

impl NodeToolDyn {
    fn new(spec: NodeToolSpec) -> Self {
        Self { spec }
    }
}

impl ToolDyn for NodeToolDyn {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn definition<'a>(&'a self, _prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        let definition = ToolDefinition {
            name: self.spec.name.clone(),
            description: self.spec.description.clone(),
            parameters: self.spec.parameters.clone(),
        };
        Box::pin(async move { definition })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> WasmBoxedFuture<'a, std::result::Result<String, ToolError>> {
        Box::pin(async move {
            let args_value = serde_json::from_str::<Value>(&args).map_err(ToolError::JsonError)?;
            let result = self
                .spec
                .callback
                .call_async::<Value>(args_value)
                .await
                .map_err(|err| {
                    tool_call_error(format!("node tool '{}' error: {err}", self.spec.name))
                })?;

            let output = if result.is_null() {
                Value::Null
            } else {
                result
            };
            serde_json::to_string(&output).map_err(ToolError::JsonError)
        })
    }
}

#[napi(js_name = "DeepAgent")]
pub struct DeepAgent {
    inner: RuntimeDeepAgent,
    hook_executor: Arc<NodeHookExecutor>,
    tool_registry: Arc<NodeToolRegistry>,
}

#[napi]
impl DeepAgent {
    #[napi(js_name = "registerHook")]
    pub fn register_hook(&self, event: String, callback: JsFunction) -> Result<()> {
        let event = parse_hook_event(&event)?;
        self.hook_executor.register(event, callback)
    }

    #[napi(js_name = "registerTool")]
    pub fn register_tool(
        &self,
        name: String,
        description: String,
        parameters: Value,
        callback: JsFunction,
    ) -> Result<()> {
        self.tool_registry
            .register(&name, &description, parameters, callback)
    }

    #[napi(js_name = "startSession")]
    pub async fn start_session(&self, init: Option<Value>) -> Result<AgentSession> {
        let init = from_json::<SessionInit>(init)?;
        let session = self
            .inner
            .start_session(init)
            .await
            .map_err(|err| Error::from_reason(err.to_string()))?;
        Ok(AgentSession { inner: session })
    }
}

#[napi(js_name = "AgentSession")]
pub struct AgentSession {
    inner: RuntimeAgentSession,
}

#[napi]
impl AgentSession {
    #[napi(getter, js_name = "sessionId")]
    pub fn session_id(&self) -> String {
        self.inner.session_id().to_string()
    }

    #[napi]
    pub async fn invoke(&self, request: Option<Value>) -> Result<Value> {
        let request = from_json::<SessionInvokeRequest>(request)?;
        let response = self
            .inner
            .invoke(request)
            .await
            .map_err(|err| Error::from_reason(err.to_string()))?;
        serde_json::to_value(response).map_err(|err| Error::from_reason(err.to_string()))
    }
}

#[napi(js_name = "createDeepAgent")]
pub fn create_deep_agent(config: Option<Value>) -> Result<DeepAgent> {
    let config = from_json::<DeepAgentConfig>(config)?;
    let hook_executor = Arc::new(NodeHookExecutor::default());
    let tool_registry = Arc::new(NodeToolRegistry::default());
    let agent = RuntimeDeepAgent::new(config)
        .map_err(|err| Error::from_reason(err.to_string()))?
        .with_hook_executor(hook_executor.clone())
        .with_tool_factory(tool_registry.clone());
    Ok(DeepAgent {
        inner: agent,
        hook_executor,
        tool_registry,
    })
}
