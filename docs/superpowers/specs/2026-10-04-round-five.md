# Round five — moving sessions between accounts, templates

Date: 2026-10-04.

- **Move** (`A` twice on a session that accepts input): `brain_core::transcript::copy_to_account`
  copies `<session>.jsonl` and its `<session>/` folder into the same project folder name of the
  next account; the copy is resumed there with `claude --resume <id> --fork-session` in a new tab,
  then `/exit` goes to the original. Verified: a headless session's code word survived the move,
  and an interactive session started from a template, moved with `A A`, answered its code word in
  the second account while the original process ended.
- **Templates** (`brain_core::templates`): Markdown files with `name`, `folder`, `account`,
  `model` header lines and the prompt as body; `{name}` placeholders are asked for in the `⌘N`
  dialog. Edited in the user's own editor (`open <file>`). Verified end to end: a template with a
  placeholder started a Haiku session whose first prompt had the value filled in.
