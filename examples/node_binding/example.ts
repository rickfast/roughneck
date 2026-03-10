import { createDeepAgent } from './roughneck.node'

async function main() {
  const agent = createDeepAgent({})
  agent.registerHook('notification', (payload) => {
    console.log('node hook payload:', payload)
    return undefined
  })
  const session = await agent.startSession({})
  const response = await session.invoke({
    messages: [{ role: 'user', content: 'Summarize the workspace' }],
  })
  console.log(response)
}

void main()
