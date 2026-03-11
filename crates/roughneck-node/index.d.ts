export type Role = 'user' | 'assistant' | 'tool'
export type TodoStatus = 'pending' | 'done'
export type CapabilityStatus = 'disabled' | 'experimental' | 'active'
export type HookEventName =
  | 'preToolUse'
  | 'postToolUse'
  | 'notification'
  | 'stop'
  | 'subagentStop'
export type HookEventPayloadName =
  | 'PreToolUse'
  | 'PostToolUse'
  | 'Notification'
  | 'Stop'
  | 'SubagentStop'

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue }

export interface ChatMessage {
  role: Role
  content: string
  name?: string
}

export interface TodoItem {
  task: string
  status: TodoStatus
}

export interface SessionInit {
  session_id?: string
  initial_messages?: ChatMessage[]
  initial_files?: Record<string, string>
}

export interface SessionInvokeRequest {
  messages?: ChatMessage[]
}

export interface HookOutputSummary {
  messages: string[]
  outputs: JsonValue[]
  suppressed_tools: string[]
}

export interface SessionInvokeResponse {
  session_id: string
  new_messages: ChatMessage[]
  latest_assistant_message?: ChatMessage | null
  workspace_snapshot?: Record<string, string> | null
  todos: TodoItem[]
  hook_output: HookOutputSummary
  metadata: Record<string, JsonValue>
}

export interface OpenAiModelProviderConfig {
  kind: 'open_ai'
  model: string
  api_key_env?: string
}

export interface AnthropicModelProviderConfig {
  kind: 'anthropic'
  model: string
  api_key_env?: string
}

export type ModelProviderConfig =
  | OpenAiModelProviderConfig
  | AnthropicModelProviderConfig

export interface InMemoryFileSystemBackendKind {
  kind: 'in_memory'
}

export interface LocalFileSystemBackendKind {
  kind: 'local'
  root: string
}

export type FileSystemBackendKind =
  | InMemoryFileSystemBackendKind
  | LocalFileSystemBackendKind

export interface ExecuteConfig {
  enabled: boolean
  default_timeout_secs: number
  max_timeout_secs: number
}

export interface FileSystemConfig {
  backend: FileSystemBackendKind
  execute?: ExecuteConfig
  snapshot_on_response?: boolean | null
}

export interface InMemoryMemoryBackendKind {
  kind: 'in_memory'
}

export interface MemoryConfig {
  backend: InMemoryMemoryBackendKind
  short_term_limit: number
}

export interface SkillsConfig {
  enabled_skills?: string[]
  registry_paths?: string[]
}

export interface HookRule {
  matcher?: string
  command: string
  timeout_secs?: number | null
}

export interface HooksConfig {
  enabled: boolean
  timeout_secs: number
  pre_tool_use?: HookRule[]
  post_tool_use?: HookRule[]
  notification?: HookRule[]
  stop?: HookRule[]
  subagent_stop?: HookRule[]
}

export interface SubagentConfig {
  name: string
  description: string
  system_prompt: string
  model?: string | null
}

export interface SubagentsConfig {
  status?: CapabilityStatus
  max_depth?: number
  agents?: SubagentConfig[]
}

export interface McpServerConfig {
  name: string
  endpoint: string
  token?: string | null
}

export interface McpConfig {
  status?: CapabilityStatus
  servers?: McpServerConfig[]
  enable_meta_tool?: boolean
}

export interface DeepAgentConfig {
  system_prompt?: string
  model?: ModelProviderConfig
  max_turns?: number
  max_tokens?: number | null
  filesystem?: FileSystemConfig
  memory?: MemoryConfig
  skills?: SkillsConfig
  subagents?: SubagentsConfig
  mcp?: McpConfig
  hooks?: HooksConfig
}

export interface HookPayload {
  hook_event_name: HookEventPayloadName
  cwd: string
  session_id: string
  invocation_id: string
  tool_call_id?: string | null
  tool_name?: string | null
  tool_input?: JsonValue
  tool_response?: JsonValue
  tool_error?: string | null
  message?: string | null
  reason?: string | null
}

export interface HookDecision {
  blocked?: boolean
  reason?: string | null
  suppress_output?: boolean
  hook_specific_output?: JsonValue[]
  messages?: string[]
}

export type HookCallback = (payload: HookPayload) => HookDecision | null | undefined

export interface ToolSchema {
  type?: string
  description?: string
  properties?: Record<string, JsonValue>
  required?: string[]
  additionalProperties?: boolean
  [key: string]: JsonValue | undefined
}

export type ToolCallback = (input: JsonValue) => JsonValue | null | undefined

export class DeepAgent {
  registerHook(event: HookEventName, callback: HookCallback): void
  registerTool(
    name: string,
    description: string,
    parameters: ToolSchema,
    callback: ToolCallback,
  ): void
  startSession(init?: SessionInit): Promise<AgentSession>
}

export class AgentSession {
  readonly sessionId: string
  invoke(request?: SessionInvokeRequest): Promise<SessionInvokeResponse>
}

export function createDeepAgent(config?: DeepAgentConfig): DeepAgent
