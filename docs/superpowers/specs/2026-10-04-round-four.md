# Round four — PR status, conflicts, quick replies

Date: 2026-10-04.

- **PR and checks** (`brain_core::github`): `gh pr view <branch> --json number,url,state,isDraft,reviewDecision,statusCheckRollup`
  in the session's folder, every two minutes off the UI thread, skipping `main`/`master`. Check
  runs roll up by `status`/`conclusion`, status contexts by `state`. Verified on a same-repo PR
  of cli/cli; fork PRs need `owner:branch` and are not looked up (sessions push to their own
  repository).
- **Conflicts** (`brain_core::conflicts`): files from the sessions' Write/Edit/MultiEdit/
  NotebookEdit calls in their transcripts; the same absolute file in two live sessions is a
  conflict (notified once per pair), changes in the same checkout a softer hint. Separate
  worktrees never conflict.
- **Quick replies**: `quick_replies` in config.json or five defaults in the UI language; in reply
  mode `1`–`9` sends one, with the same "accepts input" check as any reply.
