from typing import Any, Callable, Dict, List, Literal, Optional, TypedDict, Union

RoleName = Literal["user", "assistant", "tool"]
TodoStatusName = Literal["pending", "done"]
CapabilityStatusName = Literal["disabled", "experimental", "active"]
HookEventName = Literal[
    "pre_tool_use",
    "post_tool_use",
    "notification",
    "stop",
    "subagent_stop",
]
HookEventPayloadName = Literal[
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "SubagentStop",
]
JsonValue = Any


class ChatMessageDict(TypedDict, total=False):
    role: RoleName
    content: str
    name: str


class TodoItemDict(TypedDict, total=False):
    task: str
    status: TodoStatusName


class SessionInitDict(TypedDict, total=False):
    session_id: str
    initial_messages: List[ChatMessageDict]
    initial_files: Dict[str, str]


class SessionInvokeRequestDict(TypedDict, total=False):
    messages: List[ChatMessageDict]


class HookOutputSummaryDict(TypedDict, total=False):
    messages: List[str]
    outputs: List[JsonValue]
    suppressed_tools: List[str]


class SessionInvokeResponseDict(TypedDict, total=False):
    session_id: str
    new_messages: List[ChatMessageDict]
    latest_assistant_message: Optional[ChatMessageDict]
    workspace_snapshot: Optional[Dict[str, str]]
    todos: List[TodoItemDict]
    hook_output: HookOutputSummaryDict
    metadata: Dict[str, JsonValue]


class OpenAiModelProviderConfigDict(TypedDict, total=False):
    kind: Literal["open_ai"]
    model: str
    api_key_env: str


class AnthropicModelProviderConfigDict(TypedDict, total=False):
    kind: Literal["anthropic"]
    model: str
    api_key_env: str


ModelProviderConfigDict = Union[
    OpenAiModelProviderConfigDict,
    AnthropicModelProviderConfigDict,
]


class InMemoryFileSystemBackendKindDict(TypedDict):
    kind: Literal["in_memory"]


class LocalFileSystemBackendKindDict(TypedDict):
    kind: Literal["local"]
    root: str


FileSystemBackendKindDict = Union[
    InMemoryFileSystemBackendKindDict,
    LocalFileSystemBackendKindDict,
]


class ExecuteConfigDict(TypedDict, total=False):
    enabled: bool
    default_timeout_secs: int
    max_timeout_secs: int


class FileSystemConfigDict(TypedDict, total=False):
    backend: FileSystemBackendKindDict
    execute: ExecuteConfigDict
    snapshot_on_response: Optional[bool]


class InMemoryMemoryBackendKindDict(TypedDict):
    kind: Literal["in_memory"]


class MemoryConfigDict(TypedDict, total=False):
    backend: InMemoryMemoryBackendKindDict
    short_term_limit: int


class SkillsConfigDict(TypedDict, total=False):
    enabled_skills: List[str]
    registry_paths: List[str]


class HookRuleDict(TypedDict, total=False):
    matcher: str
    command: str
    timeout_secs: Optional[int]


class HooksConfigDict(TypedDict, total=False):
    enabled: bool
    timeout_secs: int
    pre_tool_use: List[HookRuleDict]
    post_tool_use: List[HookRuleDict]
    notification: List[HookRuleDict]
    stop: List[HookRuleDict]
    subagent_stop: List[HookRuleDict]


class SubagentConfigDict(TypedDict, total=False):
    name: str
    description: str
    system_prompt: str
    model: Optional[str]


class SubagentsConfigDict(TypedDict, total=False):
    status: CapabilityStatusName
    max_depth: int
    agents: List[SubagentConfigDict]


class McpServerConfigDict(TypedDict, total=False):
    name: str
    endpoint: str
    token: Optional[str]


class McpConfigDict(TypedDict, total=False):
    status: CapabilityStatusName
    servers: List[McpServerConfigDict]
    enable_meta_tool: bool


class DeepAgentConfigDict(TypedDict, total=False):
    system_prompt: str
    model: ModelProviderConfigDict
    max_turns: int
    max_tokens: Optional[int]
    filesystem: FileSystemConfigDict
    memory: MemoryConfigDict
    skills: SkillsConfigDict
    subagents: SubagentsConfigDict
    mcp: McpConfigDict
    hooks: HooksConfigDict


class HookPayloadDict(TypedDict, total=False):
    hook_event_name: HookEventPayloadName
    cwd: str
    session_id: str
    invocation_id: str
    tool_call_id: Optional[str]
    tool_name: Optional[str]
    tool_input: JsonValue
    tool_response: JsonValue
    tool_error: Optional[str]
    message: Optional[str]
    reason: Optional[str]


class HookDecisionDict(TypedDict, total=False):
    blocked: bool
    reason: Optional[str]
    suppress_output: bool
    hook_specific_output: List[JsonValue]
    messages: List[str]


HookCallback = Callable[[HookPayloadDict], Optional[HookDecisionDict]]


class DeepAgent:
    def register_hook(self, event: HookEventName, callback: HookCallback) -> None: ...
    def start_session(self, init: Optional[SessionInitDict] = ...) -> AgentSession: ...


class AgentSession:
    @property
    def session_id(self) -> str: ...
    def invoke(
        self, request: Optional[SessionInvokeRequestDict] = ...
    ) -> SessionInvokeResponseDict: ...


def create_deep_agent(config: Optional[DeepAgentConfigDict] = ...) -> DeepAgent: ...


__all__: List[str]
