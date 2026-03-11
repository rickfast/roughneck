const { createDeepAgent } = require('..')

async function main() {
  const agent = createDeepAgent({})
  agent.registerHook('notification', () => {
    console.log('node hook registered')
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
      console.log('node tool registered', input)
      return { version: '0.1.0' }
    },
  )

  const session = await agent.startSession({
    initial_messages: [{ role: 'user', content: 'hello' }],
  })
  if (!session.sessionId) {
    throw new Error('missing sessionId on session')
  }
  console.log('node runtime smoke passed')
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
