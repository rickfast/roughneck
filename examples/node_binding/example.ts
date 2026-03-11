import { createDeepAgent } from './roughneck.node'

async function main() {
  const agent = createDeepAgent({})
  agent.registerHook('notification', (payload) => {
    console.log('node hook payload:', payload)
    return undefined
  })
  agent.registerTool(
    'lookup_release',
    'Return a canned release version for a package name.',
    {
      type: 'object',
      properties: { name: { type: 'string' } },
      required: ['name'],
    },
    (input) => {
      if (input && typeof input === 'object' && !Array.isArray(input)) {
        return { version: '0.1.0', package: input['name'] ?? null }
      }
      return null
    },
  )
  const session = await agent.startSession({})
  const response = await session.invoke({
    messages: [{ role: 'user', content: 'Summarize the workspace' }],
  })
  console.log(response)
}

void main()
