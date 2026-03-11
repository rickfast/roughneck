use async_trait::async_trait;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyModule};
use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;
use roughneck_core::{
    DeepAgentConfig, Result as RoughneckResult, RoughneckError, SessionInit, SessionInvokeRequest,
};
use roughneck_runtime::{
    AgentSession, DeepAgent, HookDecision, HookEvent, HookExecutor, HookManager, HookPayload,
    HookedToolDyn, ProgrammaticToolFactory, ToolRuntimeContext,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, OnceLock, RwLock};

fn py_runtime_error(message: impl Into<String>) -> PyErr {
    PyRuntimeError::new_err(message.into())
}

fn py_value_error(message: impl Into<String>) -> PyErr {
    PyValueError::new_err(message.into())
}

fn runtime() -> PyResult<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    Ok(RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize Python binding tokio runtime")
    }))
}

fn from_python<T>(value: Option<&Bound<'_, PyAny>>) -> PyResult<T>
where
    T: DeserializeOwned + Default,
{
    match value {
        Some(value) => from_python_bound(value),
        None => Ok(T::default()),
    }
}

fn from_python_bound<T>(value: &Bound<'_, PyAny>) -> PyResult<T>
where
    T: DeserializeOwned,
{
    let py = value.py();
    let json = PyModule::import(py, "json")?;
    let dumped: String = json.call_method1("dumps", (value,))?.extract()?;
    serde_json::from_str(&dumped).map_err(|err| py_value_error(err.to_string()))
}

fn to_python(py: Python<'_>, value: &impl Serialize) -> PyResult<PyObject> {
    let json = PyModule::import(py, "json")?;
    let dumped = serde_json::to_string(value).map_err(|err| py_runtime_error(err.to_string()))?;
    let loaded = json.call_method1("loads", (dumped,))?;
    Ok(loaded.into())
}

fn parse_hook_event(value: &str) -> PyResult<HookEvent> {
    HookEvent::from_name(value).ok_or_else(|| {
        py_value_error(format!(
            "unknown hook event '{value}', expected one of: pre_tool_use, post_tool_use, notification, stop, subagent_stop"
        ))
    })
}

fn tool_call_error(message: impl Into<String>) -> ToolError {
    ToolError::ToolCallError(Box::new(io::Error::other(message.into())))
}

#[derive(Default)]
struct PythonHookExecutor {
    callbacks: RwLock<HashMap<HookEvent, Vec<Py<PyAny>>>>,
}

impl std::fmt::Debug for PythonHookExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let callback_count = self
            .callbacks
            .read()
            .map(|callbacks| callbacks.values().map(Vec::len).sum::<usize>())
            .unwrap_or_default();
        f.debug_struct("PythonHookExecutor")
            .field("callback_count", &callback_count)
            .finish()
    }
}

impl PythonHookExecutor {
    fn register(&self, event: HookEvent, callback: Py<PyAny>) -> PyResult<()> {
        let mut callbacks = self
            .callbacks
            .write()
            .map_err(|err| py_runtime_error(format!("python hook registry poisoned: {err}")))?;
        callbacks.entry(event).or_default().push(callback);
        Ok(())
    }

    fn callbacks_for(&self, event: HookEvent) -> RoughneckResult<Vec<Py<PyAny>>> {
        let callbacks = self.callbacks.read().map_err(|err| {
            RoughneckError::Runtime(format!("python hook registry poisoned: {err}"))
        })?;
        Python::with_gil(|py| {
            Ok(callbacks
                .get(&event)
                .map(|callbacks| {
                    callbacks
                        .iter()
                        .map(|callback| callback.clone_ref(py))
                        .collect()
                })
                .unwrap_or_default())
        })
    }
}

#[async_trait]
impl HookExecutor for PythonHookExecutor {
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
        let mut aggregate = HookDecision::default();

        for callback in callbacks {
            let decision = Python::with_gil(|py| -> PyResult<HookDecision> {
                let payload_obj = to_python(py, &payload)?;
                let result = callback.bind(py).call1((payload_obj,))?;
                if result.is_none() {
                    return Ok(HookDecision::default());
                }
                from_python_bound(&result)
            })
            .map_err(|err| RoughneckError::Runtime(format!("python hook error: {err}")))?;

            aggregate.merge(decision);
            if aggregate.blocked {
                break;
            }
        }

        Ok(aggregate)
    }
}

struct PythonToolSpec {
    name: String,
    description: String,
    parameters: Value,
    callback: Py<PyAny>,
}

impl std::fmt::Debug for PythonToolSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PythonToolSpec")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl Clone for PythonToolSpec {
    fn clone(&self) -> Self {
        Python::with_gil(|py| Self {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            callback: self.callback.clone_ref(py),
        })
    }
}

#[derive(Default)]
struct PythonToolRegistry {
    tools: RwLock<Vec<PythonToolSpec>>,
}

impl std::fmt::Debug for PythonToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tool_count = self
            .tools
            .read()
            .map(|tools| tools.len())
            .unwrap_or_default();
        f.debug_struct("PythonToolRegistry")
            .field("tool_count", &tool_count)
            .finish()
    }
}

impl PythonToolRegistry {
    fn register(
        &self,
        name: &str,
        description: &str,
        parameters: Value,
        callback: Py<PyAny>,
    ) -> PyResult<()> {
        if name.trim().is_empty() {
            return Err(py_value_error(
                "register_tool requires a non-empty tool name",
            ));
        }
        if description.trim().is_empty() {
            return Err(py_value_error(
                "register_tool requires a non-empty tool description",
            ));
        }

        let mut tools = self
            .tools
            .write()
            .map_err(|err| py_runtime_error(format!("python tool registry poisoned: {err}")))?;
        if tools.iter().any(|tool| tool.name == name) {
            return Err(py_value_error(format!(
                "a Python tool named '{name}' is already registered"
            )));
        }

        tools.push(PythonToolSpec {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
            callback,
        });
        Ok(())
    }

    fn snapshot(&self) -> RoughneckResult<Vec<PythonToolSpec>> {
        self.tools
            .read()
            .map(|tools| tools.clone())
            .map_err(|err| RoughneckError::Runtime(format!("python tool registry poisoned: {err}")))
    }
}

impl ProgrammaticToolFactory for PythonToolRegistry {
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
                    Arc::new(PythonToolDyn::new(tool)),
                    hooks.clone(),
                    runtime.clone(),
                )) as Box<dyn ToolDyn>
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
struct PythonToolDyn {
    spec: PythonToolSpec,
}

impl PythonToolDyn {
    fn new(spec: PythonToolSpec) -> Self {
        Self { spec }
    }
}

impl ToolDyn for PythonToolDyn {
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
            let result_value = Python::with_gil(|py| -> PyResult<Value> {
                let args_obj = to_python(py, &args_value)?;
                let result = self.spec.callback.bind(py).call1((args_obj,))?;
                if result.is_none() {
                    return Ok(Value::Null);
                }
                from_python_bound(&result)
            })
            .map_err(|err| {
                tool_call_error(format!("python tool '{}' error: {err}", self.spec.name))
            })?;

            serde_json::to_string(&result_value).map_err(ToolError::JsonError)
        })
    }
}

#[pyclass(name = "DeepAgent")]
pub struct PyDeepAgent {
    inner: DeepAgent,
    hook_executor: Arc<PythonHookExecutor>,
    tool_registry: Arc<PythonToolRegistry>,
}

#[pymethods]
impl PyDeepAgent {
    #[pyo3(signature = (event, callback))]
    fn register_hook(&self, event: &str, callback: &Bound<'_, PyAny>) -> PyResult<()> {
        if !callback.is_callable() {
            return Err(py_value_error("register_hook requires a callable"));
        }

        let event = parse_hook_event(event)?;
        self.hook_executor
            .register(event, callback.clone().unbind())
    }

    #[pyo3(signature = (name, description, parameters, callback))]
    fn register_tool(
        &self,
        name: &str,
        description: &str,
        parameters: &Bound<'_, PyAny>,
        callback: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if !callback.is_callable() {
            return Err(py_value_error("register_tool requires a callable"));
        }

        let parameters = from_python_bound::<Value>(parameters)?;
        self.tool_registry
            .register(name, description, parameters, callback.clone().unbind())
    }

    #[pyo3(signature = (init=None))]
    fn start_session(&self, init: Option<&Bound<'_, PyAny>>) -> PyResult<PyAgentSession> {
        let init = from_python::<SessionInit>(init)?;
        let session = runtime()?
            .block_on(self.inner.start_session(init))
            .map_err(|err| py_runtime_error(err.to_string()))?;
        Ok(PyAgentSession { inner: session })
    }
}

#[pyclass(name = "AgentSession")]
pub struct PyAgentSession {
    inner: AgentSession,
}

#[pymethods]
impl PyAgentSession {
    #[getter]
    fn session_id(&self) -> String {
        self.inner.session_id().to_string()
    }

    #[pyo3(signature = (request=None))]
    fn invoke(&self, py: Python<'_>, request: Option<&Bound<'_, PyAny>>) -> PyResult<PyObject> {
        let request = from_python::<SessionInvokeRequest>(request)?;
        let response = runtime()?
            .block_on(self.inner.invoke(request))
            .map_err(|err| py_runtime_error(err.to_string()))?;
        to_python(py, &response)
    }
}

#[pyfunction]
#[pyo3(signature = (config=None))]
pub fn create_deep_agent(config: Option<&Bound<'_, PyAny>>) -> PyResult<PyDeepAgent> {
    let config = from_python::<DeepAgentConfig>(config)?;
    let hook_executor = Arc::new(PythonHookExecutor::default());
    let tool_registry = Arc::new(PythonToolRegistry::default());
    let agent = DeepAgent::new(config)
        .map_err(|err| py_runtime_error(err.to_string()))?
        .with_hook_executor(hook_executor.clone())
        .with_tool_factory(tool_registry.clone());
    Ok(PyDeepAgent {
        inner: agent,
        hook_executor,
        tool_registry,
    })
}

#[pymodule]
fn _roughneck_py(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyDeepAgent>()?;
    module.add_class::<PyAgentSession>()?;
    module.add_function(wrap_pyfunction!(create_deep_agent, module)?)?;

    let exports = vec![
        "DeepAgent".into_pyobject(py)?.into_any().unbind(),
        "AgentSession".into_pyobject(py)?.into_any().unbind(),
        "create_deep_agent".into_pyobject(py)?.into_any().unbind(),
    ];
    module.add("__all__", exports)?;
    Ok(())
}
