// Brain's mod: tells the Brain dashboard what this session does, from inside Claude Code.
// It sends the same JSON Brain's settings hooks send (`brain hook`), and the plan limits and
// context the status line wrapper recorded (`brain statusline`), through the `brain` CLI.

// Where `brain install` puts the CLI; `brain` on the PATH is tried first.
const FALLBACK = '.cargo/bin/brain'

// Runs `brain <args>`. Reporting must never disturb the session, so failures are swallowed.
async function brain($, args) {
  try {
    await $.process.run(['brain', ...args], { timeoutMs: 5000 })
    return
  } catch {}
  try {
    const home = await $.env.get('HOME')
    if (home) await $.process.run([home + '/' + FALLBACK, ...args], { timeoutMs: 5000 })
  } catch {}
}

// One event as a settings hook would receive it.
async function hook($, name, fields) {
  const payload = { hook_event_name: name, session_id: await $.session.id(), cwd: await $.session.cwd(), ...fields }
  await brain($, ['hook', '--json', JSON.stringify(payload)])
}

// Subagents still running when a turn ends: Brain shows the session as busy in the background.
async function runningAgents($) {
  try {
    const agents = await $.agent.list()
    return agents
      .filter((a) => ['pending', 'running', 'waiting'].includes(a.status))
      .map((a) => ({ id: String(a.id ?? a.agentId ?? ''), type: 'subagent', status: a.status, description: a.description ?? a.name }))
  } catch {
    return []
  }
}

// The plan limits and context as the status line JSON names them.
function statusline(sessionId, usage) {
  const limits = {}
  for (const limit of usage.rateLimits ?? []) {
    const resets = limit.resetsAt ? Math.floor(Date.parse(limit.resetsAt) / 1000) : undefined
    limits[limit.kind] = { used_percentage: limit.percentUsed, resets_at: resets }
  }
  return {
    session_id: sessionId,
    context_window: { used_percentage: usage.context?.percent, context_window_size: usage.context?.window },
    rate_limits: limits,
    cost: { total_cost_usd: usage.cost?.usd },
  }
}

// A hook that fails passes the event on unchanged, so reporting never holds up the session.
// After a failed hook had called next, next(e) returns that result without running it again.
const passOn = async ($, e, next) => next(e)

export function register(on) {
  on('session.start', async ($, e, next) => {
    await hook($, 'SessionStart', { source: 'startup' })
    return next(e)
  })

  on('prompt.submit', async ($, e, next) => {
    // Prompts a mod submits aren't the user's.
    if (e.origin?.kind !== 'plugin') await hook($, 'UserPromptSubmit', { prompt: e.text })
    return next(e)
  }).catch(passOn)

  // Claude asks the user something: the session needs them.
  on('tool.call', { tool: 'AskUserQuestion' }, async ($, e, next) => {
    const question = e.questions?.[0]?.question
    await hook($, 'Notification', { message: question ? 'Claude asks: ' + question : 'Claude has a question for you' })
    return next(e)
  }).catch(passOn)

  // The user is about to be asked for permission.
  on('tool.check', async ($, e, next) => {
    const decided = await next(e)
    const decision = typeof decided === 'string' ? decided : decided?.decision
    if (decision === 'ask' && !e.agentId) await hook($, 'Notification', { message: 'Claude needs your permission to use ' + e.tool })
    return decided
  }).catch(passOn)

  on('turn.complete', async ($, e, next) => {
    // Subagents' turns end inside the main one.
    if (!e.agentId) {
      await hook($, 'Stop', { last_assistant_message: e.answer ?? '', background_tasks: await runningAgents($) })
    }
    return next(e)
  })

  on('session.measure', async ($, e, next) => {
    await brain($, ['statusline', '--json', JSON.stringify(statusline(await $.session.id(), e))])
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    await hook($, 'SessionEnd', { reason: e.reason })
    return next(e)
  })
}
