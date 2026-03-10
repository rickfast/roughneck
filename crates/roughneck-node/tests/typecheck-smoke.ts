import {
  createDeepAgent,
  type DeepAgent,
  type HookDecision,
  type HookPayload,
  type SessionInvokeResponse,
} from '..'

const hook = (payload: HookPayload): HookDecision | undefined => {
  if (payload.hook_event_name === 'Notification') {
    return { messages: ['typed node hook'] }
  }
  return undefined
}

const agent: DeepAgent = createDeepAgent({
  system_prompt: 'typed smoke',
  subagents: { status: 'disabled', agents: [] },
})
agent.registerHook('notification', hook)

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
