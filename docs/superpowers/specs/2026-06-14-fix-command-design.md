# `aish fix` — run a command and diagnose its failure

Date: 2026-06-14
Status: Proposed

## Problem

aish's tagline is *"AI copilot for your command line … wraps the commands
developers run … to draft clean summaries and **troubleshoot output**."*
Every other promise has a dedicated subcommand (`commit`, `pr`, `review`,
`changelog`), but "troubleshoot output" does not. The closest tool is `ask`,
which only consumes **manually piped** stdin:

```
cargo build 2>&1 | aish ask "explain this error"
```

That has three weaknesses:

1. **Manual ceremony.** The user must remember `2>&1 | aish ask "…"` and type
   a question every time something breaks.
2. **Lost signal.** A pipe captures stdout+stderr text but throws away the
   **exit code**, and shells often interleave/lose stderr. The model never
   learns the command actually *failed* or *how*.
3. **No command context.** The model sees output but not the command that
   produced it, so it guesses at intent.

The natural copilot gesture is: a command broke, so re-run it under aish.

## Solution

Add `aish fix <cmd>…` — a thin wrapper that runs the command, streams its
real output to the terminal, and **on a nonzero exit code** appends an AI
diagnosis plus a concrete suggested fix. On success it is a transparent
pass-through (the model is not called, no tokens spent).

```
$ aish fix cargo build
   Compiling app v0.1.0
error[E0382]: borrow of moved value: `cfg`
  --> src/main.rs:42:18
   ...
─── aish ─────────────────────────────────────────────
`cfg` is moved into `spawn` on line 39, then borrowed on line 42.
Clone it before the move, or capture a reference:

    let cfg = cfg.clone();   // before the spawn
────────────────────────────────────────────────────────
$ echo $?
101
```

`fix` **diagnoses and suggests; it does not edit files or re-run anything.**
The name is the verb users reach for when something breaks; the safety
boundary (no auto-apply) is stated in `--help` and the README. A future
`--apply` is explicitly out of scope (YAGNI).

## Command shape

```
aish fix [OPTIONS] <CMD>...

  <CMD>...        The command to run, e.g. `aish fix cargo test`.
                  Captured as trailing args; flags after the command
                  belong to the command, not to aish.

  --shell         Run <CMD> via `sh -c "<joined>"` so pipes/redirects/globs
                  work (e.g. `aish fix --shell "make 2>&1 | tail"`).
                  Default: direct argv exec (no shell, safer, no word-split).
  --always        Diagnose even when the command succeeds (exit 0).
  --model <ALIAS> Override the model alias from config.
  --lang <LANG>   Override output language.
  --no-cache      Bypass the response cache.
```

Inherits the global `--json` and `--verbose` flags. Flag names and config
fallbacks mirror the existing subcommands (`commit`, `review`, `ask`) for
consistency: alias falls back to `cfg.commit.model`, language to
`cfg.commit.language`.

### Exit-code contract

`aish fix` **always propagates the wrapped command's exit code** (so it is
safe in scripts and `&&` chains). The diagnosis is purely additive output on
stderr-adjacent channel; it never changes the exit status. If aish's own
machinery fails (config invalid, provider unreachable), the command's output
is still shown and its exit code still propagated — the diagnosis is
best-effort and a model error degrades to a one-line warning, not a hard
failure that masks the real exit code.

## Architecture

Reuses the existing pipeline end to end; no new infrastructure.

- **`src/cli.rs`** — add `Command::Fix { cmd: Vec<String>, shell, always,
  model, lang, no_cache }`. Use clap `trailing_var_arg = true` +
  `allow_hyphen_values = true` on `cmd` so `aish fix cargo build --release`
  routes `--release` to cargo.
- **`src/tool/fix.rs`** (new) — `build_messages(language, command,
  exit_code, output) -> Vec<Message>`. System prompt: a terminal copilot
  that explains why a command failed and gives one concrete, minimal fix;
  plain text / minimal markdown; no outer fence. User message embeds the
  command string, the exit code, and the captured output.
- **`src/commands/fix.rs`** (new) — `run(...)`:
  1. Spawn the command (direct argv, or `sh -c` under `--shell`), **teeing**
     combined stdout+stderr: write through to the user's terminal live *and*
     accumulate into a buffer. Capture the exit code.
  2. If exit code == 0 and not `--always`: return that exit code, done.
  3. Otherwise: load config, resolve model, `build_messages`, call
     `commands::generate::generate`, `tool::review::postprocess`, print the
     diagnosis in a delimited block (or `--json`).
  4. `audit::record` with tool `fix.diagnose`, decision `diagnosed` /
     `skipped`.
  5. Return the wrapped command's exit code.
- **`src/commands/mod.rs`** — register module + dispatch arm.

### Output capture detail

Merge stdout and stderr into one ordered stream (so the model sees errors in
context) and tee it: the user sees output exactly as if they ran the command,
and aish keeps a copy. Acceptable simplification for v1: capture combined
output via a piped child and echo lines as they arrive; full PTY/TTY
fidelity (colors, interactive prompts) is out of scope — `fix` targets
non-interactive, failing build/test/lint commands.

### Truncation — opposite of diffs

Reuse `MAX_DIFF_CHARS` (12 000) but **keep the tail, not the head**: a
failure message and stack trace live at the *end* of output, whereas a diff's
signal is uniform. Add `truncate_tail(s, marker)` in `tool/commit.rs`
alongside the existing head-truncation helper, cutting on a UTF-8 boundary and
prefixing `[earlier output truncated]`.

## Testing

`tests/fix_e2e.rs`, `AISH_PROVIDER=mock`, no network:

1. **Failure is diagnosed.** Run a command that exits nonzero (`aish fix --
   sh -c "exit 1"` or a tiny helper script). Assert the diagnosis block is
   printed and the process exits with the wrapped code (1).
2. **Success passes through.** `aish fix sh -c "exit 0"` (without `--always`)
   prints no diagnosis, exits 0, makes no provider call.
3. **`--always` diagnoses success.** Same succeeding command + `--always`
   prints a diagnosis, still exits 0.
4. **Exit code propagates.** A command exiting 42 makes `aish fix` exit 42.
5. **`--json`** emits the diagnosis + provider/model/token fields, matching
   the envelope used by `ask`/`review`.

Unit tests in `tool/fix.rs`: prompt contains the command, the exit code, and
the captured output; tail-truncation keeps the end and inserts the marker.

## Scope boundaries (YAGNI)

- **No** auto-apply / `--apply` / file edits.
- **No** retry loop or "fix then re-run".
- **No** interactive/PTY fidelity; non-interactive commands only.
- **No** new config section; reuses `commit.model` / `commit.language`.
- **No** plugin hook — built-in subcommand, per ADR-0001.
