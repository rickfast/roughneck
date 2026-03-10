const { createDeepAgent } = require('..')

async function main() {
  const agent = createDeepAgent({})
  agent.registerHook('notification', () => {
    console.log('node hook registered')
    return undefined
  })

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
