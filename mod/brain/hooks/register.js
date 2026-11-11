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

// What Brain asks of this session (a reply, a slash command, an interrupt), so nothing has to be
// typed into a terminal. Brain writes `~/.claude-brain/requests/<session id>.json`:
// `{ items: [{ seq, kind: 'prompt', text } | { seq, kind: 'interrupt' }] }`. The mod takes each
// `seq` once and writes the last one it took to `<session id>.done`; that file also tells Brain
// this session takes requests.
let handled = 0
let taking = false
let runningTurn

async function requestFiles($) {
  const home = await $.env.get('HOME')
  const id = await $.session.id()
  if (!home || !id) return undefined
  const base = home + '/.claude-brain/requests/' + id
  return { requests: base + '.json', done: base + '.done' }
}

async function readItems($, file) {
  try {
    if (!(await $.fs.exists(file))) return []
    const items = JSON.parse(await $.fs.read(file)).items
    return Array.isArray(items) ? items : []
  } catch {
    return []
  }
}

// At the start: what was already there (a resumed session's old requests) is not done again.
async function openRequests($) {
  const files = await requestFiles($)
  if (!files) return
  let done = 0
  try {
    if (await $.fs.exists(files.done)) done = Number((await $.fs.read(files.done)).trim()) || 0
  } catch {}
  const items = await readItems($, files.requests)
  handled = Math.max(done, ...items.map((item) => Number(item.seq) || 0))
  await $.fs.write(files.done, String(handled))
  $.clock.every(500, () => void takeRequests($))
}

async function takeRequests($) {
  if (taking) return
  taking = true
  try {
    const files = await requestFiles($)
    if (!files) return
    const items = (await readItems($, files.requests)).filter((item) => Number(item.seq) > handled)
    for (const item of items.sort((a, b) => a.seq - b.seq)) {
      handled = Number(item.seq)
      await $.fs.write(files.done, String(handled))
      try {
        await carryOut($, item)
      } catch {}
    }
  } catch {
  } finally {
    taking = false
  }
}

async function carryOut($, item) {
  if (item.kind === 'interrupt') {
    if (runningTurn !== undefined) await $.turn.abort({ turnId: runningTurn })
    return
  }
  if (item.kind !== 'prompt' || typeof item.text !== 'string' || !item.text.trim()) return
  // Queued: it starts a turn once the session is idle, as the person's own words. The mod's own
  // `prompt.submit` hook doesn't see what it submits itself, so the prompt is reported here, at
  // once: a reload of the mod while the prompt waits for its turn would lose a later report.
  const submit = () => {
    void hook($, 'UserPromptSubmit', { prompt: item.text })
    void $.prompt.submit({ text: item.text, asUser: true }).catch(() => {})
  }
  // `/compact`, `/rename …`: the session's own command, as if typed. One the engine doesn't run
  // for a plugin goes as typed text, which Claude Code reads as the command.
  const slash = /^\/(\S+)\s*([\s\S]*)$/.exec(item.text.trim())
  if (slash) {
    void $.command.run({ command: slash[1], args: slash[2] }).catch(submit)
    return
  }
  submit()
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
    try {
      await openRequests($)
    } catch {}
    return next(e)
  })

  // The main loop's running turn, for an interrupt from Brain.
  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) runningTurn = e.turnId
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
    // Prompts a mod submits aren't the user's (Brain's own requests report themselves).
    if (e.origin?.kind !== 'plugin') {
      await hook($, 'UserPromptSubmit', { prompt: e.text })
    }
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
        // Options with a preview: the dialog picks one with Return, not with its number alone.
        previews: (q.options ?? []).some((o) => o.preview != null),
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
      runningTurn = undefined
      await hook($, 'Stop', { last_assistant_message: e.answer ?? '', background_tasks: await runningAgents($) })
    }
    return next(e)
  })

  on('session.measure', async ($, e, next) => {
    await handOver($, 'statusline', statusline(await $.session.id(), e))
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    await hook($, 'SessionEnd', { reason: e.reason })
    return next(e)
  })
}
