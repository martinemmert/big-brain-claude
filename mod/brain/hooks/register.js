// Brain's mod: tells the Brain dashboard what this session does, from inside Claude Code.
// It sends the same JSON Brain's settings hooks send (`brain hook`), and the plan limits and
// context the status line wrapper recorded (`brain statusline`), through the `brain` CLI.
// The JSON holds prompts and answers, so it goes through a file in Brain's inbox, a folder only
// the user can open, never as an argument: arguments show in the process list.

// Where `brain install` puts the CLI; `brain` on the PATH is tried first.
const FALLBACK = '.cargo/bin/brain'

// Leaves the JSON in the inbox and runs `brain <command> --json-file <file>`, which reads and
// deletes it.
async function handOver($, command, value) {
  try {
    const home = await $.env.get('HOME')
    if (!home) return
    const file = home + '/.claude-brain/inbox/' + crypto.randomUUID() + '.json'
    await $.fs.write(file, JSON.stringify(value))
    await brain($, [command, '--json-file', file])
  } catch {}
}

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
  await handOver($, 'hook', payload)
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

// The slash commands the person can run in this session (built-in, skills, plugins, MCP), for
// completing `/…` in Brain and on the phone.
async function reportCommands($) {
  try {
    const commands = (await $.command.list()).map((c) => ({ name: c.name, description: c.description ?? '', source: c.source }))
    await handOver($, 'commands', { session_id: await $.session.id(), commands })
  } catch {}
}

// Agents of this session (an agent team's members, subagents) and what they write each other,
// for Brain's Team tab. Messages asking the person ("Frage an Martin: …") are questions there.
const LEAD = 'lead'
const QUESTION = /frage an martin/i
let teamMessages = []

async function reportTeam($) {
  try {
    const members = (await $.agent.list()).map((a) => ({
      id: String(a.id),
      name: a.name ?? a.description ?? String(a.id).slice(0, 8),
      type: a.type ?? '',
      status: a.status ?? '',
      teammate: a.teammateId ?? null,
    }))
    if (members.length === 0 && teamMessages.length === 0) return
    await handOver($, 'team', { session_id: await $.session.id(), members, messages: teamMessages })
  } catch {}
}

async function nameOfLoop($, agentId) {
  if (agentId === undefined) return LEAD
  const agent = (await $.agent.list()).find((one) => one.id === agentId)
  return agent?.name ?? agent?.description ?? String(agentId).slice(0, 8)
}

// In an agent team the lead is `team-lead`: the same as the main conversation here.
const member = (name) => (name === 'team-lead' ? LEAD : name)

async function recordMessage($, sender, recipient, text) {
  const from = member(sender)
  const to = member(recipient)
  const at = await $.clock.now()
  // A message from the lead to someone answers that someone's open questions.
  if (from === LEAD) teamMessages = teamMessages.map((m) => (m.question && m.from === to ? { ...m, answered: true } : m))
  teamMessages = [...teamMessages, { id: `${at}-${from}-${to}`, from, to, text: text.trim().slice(0, 2000), at, question: QUESTION.test(text), answered: false }].slice(-200)
  await reportTeam($)
}

// A hook that fails passes the event on unchanged, so reporting never holds up the session.
// After a failed hook had called next, next(e) returns that result without running it again.
const passOn = async ($, e, next) => next(e)

export function register(on) {
  on('session.start', async ($, e, next) => {
    await hook($, 'SessionStart', { source: 'startup' })
    try {
      await $.command.register({
        name: 'later',
        description: 'Note a task for later in Brain (with this folder)',
        argumentHint: '[task]',
        immediate: true,
      })
    } catch {}
    await reportCommands($)
    return next(e)
  })

  // `/later <task>`: Brain keeps it with this session's folder, at once and without the model.
  on('command.run', { command: 'later' }, async ($, e) => {
    const text = (e.args ?? '').trim()
    if (!text) return { text: 'Usage: /later <task> – Brain lists it, starts it as a new session or checks it off.' }
    await handOver($, 'later', { session_id: await $.session.id(), cwd: await $.session.cwd(), text })
    return { text: 'Noted for later in Brain: ' + text }
  })

  on('prompt.submit', async ($, e, next) => {
    // Prompts a mod submits aren't the user's.
    if (e.origin?.kind !== 'plugin') await hook($, 'UserPromptSubmit', { prompt: e.text })
    return next(e)
  }).catch(passOn)

  // Claude asks the user something: the session needs them. The question goes along with its
  // options, for the phone: Claude Code writes the call to the transcript only with its answer.
  on('tool.call', { tool: 'AskUserQuestion' }, async ($, e, next) => {
    const questions = e.questions ?? []
    const question = questions[0]?.question
    const ask = {
      call: e.tool_use_id,
      items: questions.map((q) => ({
        question: q.question ?? '',
        options: (q.options ?? []).map((o) => o.label ?? ''),
        multi_select: q.multiSelect === true,
      })),
      multi_select: questions.some((q) => q.multiSelect === true),
    }
    await hook($, 'Notification', { message: question ? 'Claude asks: ' + question : 'Claude has a question for you', ask })
    return next(e)
  }).catch(passOn)

  // Claude Code tells the user it needs them: a permission dialog, or the idle reminder. (A
  // `tool.check` verdict of `ask` is not that: in auto mode the classifier decides most of them.)
  on('classic.Notification', async ($, e, next) => {
    await hook($, 'Notification', { message: e.message, notification_type: e.notification_type })
    return next(e)
  }).catch(passOn)

  on('turn.complete', async ($, e, next) => {
    // Subagents' turns end inside the main one.
    if (!e.agentId) {
      await hook($, 'Stop', { last_assistant_message: e.answer ?? '', background_tasks: await runningAgents($) })
    }
    // Who works and who waits now, the lead's turn and each member's.
    await reportTeam($)
    return next(e)
  })

  // What the session's agents write each other (the lead is the main conversation).
  on('session.send', async ($, e, next) => {
    const result = await next(e)
    if (result.isDelivered) await recordMessage($, await nameOfLoop($, e.agentId), String(e.to).split('@')[0], e.text ?? '')
    return result
  }).catch(passOn)

  // A teammate in a pane of its own sends from its own process: only its delivery shows here.
  on('session.receive', async ($, e, next) => {
    const result = await next(e)
    const origin = e.origin ?? {}
    if ('teammate' in origin && !origin.isVerified && result.consumed === undefined) {
      await recordMessage($, String(origin.teammate).split('@')[0], await nameOfLoop($, e.agentId), e.text ?? '')
    }
    return result
  }).catch(passOn)

  on('session.measure', async ($, e, next) => {
    await handOver($, 'statusline', statusline(await $.session.id(), e))
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    await hook($, 'SessionEnd', { reason: e.reason })
    return next(e)
  })
}
