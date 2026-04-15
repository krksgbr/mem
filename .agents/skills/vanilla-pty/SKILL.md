---
name: vanilla-pty
description: Drive interactive terminal programs directly with the built-in PTY tools (`exec_command` + `write_stdin`) when you need live TUI interaction without an external helper like tui-use.
---

# vanilla-pty — Direct PTY Workflow for Agents

Use this skill when you need to interact with a terminal program directly through the built-in PTY tools:

- `functions.exec_command` with `tty: true`
- `functions.write_stdin`

This is the fallback and baseline workflow when:

- you need live TUI interaction
- `dump-screen` is not enough
- `tui-use` is unavailable or unreliable
- you want full control over the PTY lifecycle from the current agent session

For `transcript-browser`, this workflow is useful for reproducing:

- delayed screen transitions
- focus/navigation bugs
- background-sync interactions
- keybinding behavior

It is **not** the primary verification path. Use it for diagnosis, then convert findings into:

- a regression test
- a `dump-screen` repro
- a screen ref

## Core Workflow

```
start PTY → send keys → poll output → inspect screen/state → repeat → quit cleanly
```

## Start the PTY

Use `exec_command` with:

- `tty: true`
- a long enough `yield_time_ms` to let the app render
- the project `workdir`

For this repo, prefer:

```text
just run --profile
```

Example shape:

```json
{
  "cmd": "just run --profile",
  "workdir": "/Users/gaborkerekes/projects/transcript-browser",
  "tty": true,
  "yield_time_ms": 1200
}
```

This returns a `session_id` for continued interaction.

## Send Input

Use `write_stdin` against the PTY session.

Examples:

- Down arrow:
  - `"\u001b[B"`
- Up arrow:
  - `"\u001b[A"`
- Right arrow:
  - `"\u001b[C"`
- Left arrow:
  - `"\u001b[D"`
- Enter:
  - `"\r"`
- Quit:
  - `"q"`

Example:

```json
{
  "session_id": 12345,
  "chars": "\u001b[B\r",
  "yield_time_ms": 1200
}
```

That example means:

- move down one row
- press Enter
- wait for the next redraw

## Poll, Don’t Stack Blindly

After starting a PTY command or sending input:

- poll the same session with `write_stdin` and empty `chars`
- inspect the returned terminal output before sending more input

Example:

```json
{
  "session_id": 12345,
  "chars": "",
  "yield_time_ms": 1000
}
```

This avoids stacking multiple assumptions on top of stale screen state.

## Recommended Pattern

1. Start the app in a PTY.
2. Send one small input step.
3. Poll until you see the next stable render.
4. Inspect what screen you are actually on.
5. Repeat.

For navigation bugs, prefer:

- one key at a time
- short sequences only when you already know the exact starting state

## Transcript-Browser Specific Guidance

### Preferred launch command

Use:

```text
just run --profile
```

Reasons:

- it matches normal developer usage in this repo
- it writes `transcript-browser-profile.json`
- if the app fails or bounces unexpectedly, the profile can help explain why

### Do not use raw `cargo run` casually

This repo’s AGENTS guidance already says:

- do not use the interactive TUI as the normal verification path
- prefer tests, `dump-screen`, and screen refs first

Direct PTY driving is for cases where interactive behavior itself is the bug.

### Key sequences we used successfully

- open the second workspace:
  - `"\u001b[B\r"`
- quit:
  - `"q"`

### Expect ANSI-heavy output

The PTY output will include terminal escape sequences.

That is normal.

Use it for:

- detecting which screen rendered
- checking whether selection moved
- confirming whether the app changed screens unexpectedly

Do not expect it to be as clean as `dump-screen` or `tui-use` JSON snapshots.

## Practical Rules

1. Start small.
   Use minimal key sequences so you can attribute each transition to one input.

2. Poll after each step.
   Do not assume the app is on the screen you expect; confirm it from output.

3. Prefer anchors over raw counts.
   If you need a specific target, avoid long blind sequences of arrow keys.

4. Quit cleanly.
   Send `q` when done so the PTY session exits and `--profile` can flush its report.

5. Convert discoveries into deterministic artifacts.
   PTY interaction is for exploration. The durable artifact should be a test, `dump-screen` repro, or screen ref.

## Example Session

Start:

```text
exec_command({
  cmd: "just run --profile",
  workdir: "/Users/gaborkerekes/projects/transcript-browser",
  tty: true,
  yield_time_ms: 1200
})
```

Open the second workspace:

```text
write_stdin({
  session_id: <SESSION>,
  chars: "\u001b[B\r",
  yield_time_ms: 1200
})
```

Poll:

```text
write_stdin({
  session_id: <SESSION>,
  chars: "",
  yield_time_ms: 1000
})
```

Quit:

```text
write_stdin({
  session_id: <SESSION>,
  chars: "q",
  yield_time_ms: 1200
})
```

## When to Use Something Else

Use a different tool when:

- you only need one exact render:
  - use `dump-screen`
- you need a reproducible human-shared view:
  - use a screen ref
- you need clean structured PTY snapshots with highlights:
  - use `tui-use` if it is working in the environment
- you need stable verification:
  - use tests
