import {
  createDeepAgent,
  type DeepAgent,
  type HookDecision,
  type HookPayload,
  type JsonValue,
  type SessionInvokeResponse,
  type ToolSchema,
} from '..'

const hook = (payload: HookPayload): HookDecision | undefined => {
  if (payload.hook_event_name === 'Notification') {
    return { messages: ['typed node hook'] }
  }
  return undefined
}

const releaseToolSchema: ToolSchema = {
  type: 'object',
  properties: {
    name: { type: 'string' },
  },
  required: ['name'],
}

const lookupRelease = (input: JsonValue): JsonValue => {
  if (input && typeof input === 'object' && !Array.isArray(input)) {
    const record = input as Record<string, JsonValue>
    if (record.name === 'roughneck') {
      return { version: '0.1.0' }
    }
  }
  return null
}

const agent: DeepAgent = createDeepAgent({
  system_prompt: 'typed smoke',
  subagents: { status: 'disabled', agents: [] },
})
agent.registerHook('notification', hook)
agent.registerTool(
  'lookup_release',
  'Return a canned release version for a package name.',
  releaseToolSchema,
  lookupRelease,
)

async function main(): Promise<void> {
  const session = await agent.startSession({
    initial_messages: [{ role: 'user', content: 'hello' }],
  })
  const response: SessionInvokeResponse = await session.invoke({
    messages: [{ role: 'user', content: 'world' }],
  })
  const latest = response.latest_assistant_message
  if (latest) {
    const role = latest.role
    console.log(role)
  }
}

void main()
