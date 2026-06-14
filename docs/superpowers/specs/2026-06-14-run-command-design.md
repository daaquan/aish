# `aish run` — natural language to shell command

Date: 2026-06-14
Status: Accepted

## Problem

aish's binary description is *"AI-powered extensible shell for developers,"* yet
no subcommand turns natural language into a shell command. Every other tagline
promise has a dedicated tool — `commit`, `pr`, `review`, `changelog`,
`ask`, `fix` — but the "shell" promise is unfulfilled. The closest tool is
`ask`, which only returns prose; the user must read it, extract the command,
and retype it. The natural copilot gesture is the inverse of `fix`:

- `fix` — a command broke; explain why and suggest a fix.
- `run` — the user knows the intent but not the incantation; produce the
  command and offer to run it.

## Solution

Add `aish run <PROMPT>…` — describe the intent in natural language, the model
emits exactly one shell command (grounded in the current OS and shell), and
aish shows it behind a confirm/edit gate before executing.

```
$ aish run delete all merged git branches
─── aish ─────────────────────────────────────────────
git branch --merged | grep -v '\*' | xargs -r git branch -d
────────────────────────────────────────────────────────
[y]es run · [n]o · [e]dit ▸ y
Deleted branch feature/old (was 1a2b3c4).
```

Confirm-then-run is the safety boundary: generating and running an arbitrary
shell command has a high blast radius, so it never executes without an explicit
gate. `--yes` is the only path to no-prompt execution and is documented as
such. `--print` emits the command to stdout and never executes, for piping or
manual review.

## Command shape

```
aish run [OPTIONS] <PROMPT>...

  <PROMPT>...      Natural-language description of the desired command, e.g.
                  `aish run compress every .log file in this directory`.
                  Captured as trailing args and joined with spaces.

  --yes, -y       Skip the confirm prompt and run the command immediately.
                  The only path to no-prompt execution.
  --print         Print the generated command to stdout and exit without
                  running it (pipe-friendly; no confirm prompt).
  --model <ALIAS> Override the model alias from config.
  --lang <LANG>   Override output language (affects the confirm-prompt
                  wording only; the command itself is shell, not prose).
  --no-cache      Bypass the response cache.
```

Inherits the global `--json` and `--verbose` flags. Model and language fall
back to `cfg.commit.model` / `cfg.commit.language`, mirroring `fix`, `ask`,
and `review` for consistency. If both `--print` and `--yes` are passed,
`--print` takes precedence and `--yes` is ignored: `--print` never executes,
so the no-prompt gate is moot.

### Exit-code contract

When the command runs, `aish run` **propagates the wrapped command's exit
code** (safe in scripts and `&&` chains). When the user aborts at the confirm
prompt, or `--print` is used, `aish run` exits 0. If command generation itself
fails (config invalid, provider unreachable, empty model output), aish exits
nonzero with a one-line error and runs nothing.

## Architecture

Reuses the existing pipeline end to end; no new infrastructure.

- **`src/cli.rs`** — add `Command::Run { prompt: Vec<String>, yes, print,
  model, lang, no_cache }`. Use clap `trailing_var_arg = true` on `prompt`
  so the whole description is captured without quoting ceremony.
- **`src/tool/run.rs`** (new) — `build_messages(language, os, shell, prompt)
  -> Vec<Message>`. System prompt: a terminal copilot that emits **exactly
  one** shell command for `{shell}` on `{os}`, no prose, no explanation, no
  code fence. User message is the natural-language prompt. Add
  `postprocess(reply) -> Option<String>`: trim, strip a surrounding code
  fence / backticks if present, and reject (return `None`) if the result is
  empty or contains a newline (multi-command output is out of scope). Tool
  `name()` is `command.generate` (distinct from `fix`'s `command.diagnose`).
- **`src/commands/run.rs`** (new) — `run(...)`:
  1. Load config, resolve model, detect OS (`std::env::consts::OS`) and shell
     (`$SHELL` basename, default `sh`), `build_messages`, call
     `commands::generate::generate`.
  2. `postprocess` the reply. On `None`: print a one-line error, exit nonzero,
     run nothing.
  3. If `--print`: print the command to stdout, `audit::record` decision
     `printed`, exit 0.
  4. If `--yes`: skip to step 6 with the generated command.
  5. Otherwise run the confirm/edit loop (reuse the commit flow): show the
     command in a delimited block, prompt `[y]es / [n]o / [e]dit`. `n` →
     record `aborted`, exit 0. `e` → open the command in `$EDITOR`, use the
     edited text. `y` → proceed.
  6. Execute via `sh -c "<command>"` (natural-language commands routinely need
     pipes, globs, and `&&`, so shell interpretation is the default and only
     mode). Tee combined stdout+stderr to the terminal.
  7. `audit::record` with tool `command.generate`, decision `ran` / `edited`.
  8. Return the wrapped command's exit code.
- **`src/commands/mod.rs`** — register module + dispatch arm.

### Why `sh -c` and not direct argv

`fix` defaults to direct argv exec because the user typed the command and any
shell features are explicit. `run` is the opposite: the model produces the
command, and useful commands almost always use shell features (`|`, `>`,
`*`, `&&`). Direct argv would break the common case, so `run` always uses
`sh -c`. This is exactly why the confirm gate matters and is the default.

### Output capture detail

Same simplification as `fix`: merge stdout and stderr into one ordered stream
and tee it — the user sees output as if they ran the command directly, no PTY
fidelity (colors, interactive prompts). `run` targets non-interactive,
one-shot commands.

## Testing

`tests/run_e2e.rs`, `AISH_PROVIDER=mock`, no network:

1. **`--print` emits, never runs.** `aish run --print "list files"` prints the
   mock-generated command to stdout, makes no shell execution, exits 0.
2. **Abort runs nothing.** Confirm prompt answered `n` aborts; no command runs,
   exit 0.
3. **`--yes` runs and propagates.** `aish run --yes "<mock cmd that exits 42>"`
   runs the command and exits 42.
4. **Generation failure.** Mock returns empty/multi-line → aish prints an error,
   runs nothing, exits nonzero.
5. **`--json`** emits the generated command + provider/model/token fields,
   matching the envelope used by `ask` / `review` / `fix`.

Unit tests in `tool/run.rs`: prompt embeds the OS and shell; `postprocess`
strips a surrounding fence/backticks, trims, and rejects empty or multi-line
replies.

## Scope boundaries (YAGNI)

- **No** multi-command scripts — single-line commands only (v1).
- **No** command history, recall, or learning from past runs.
- **No** cwd file listing in the prompt — keeps the prompt small and avoids
  leaking file names to the provider; cwd-specific intent still works through
  the natural-language description.
- **No** retry / "didn't work, try again" loop.
- **No** new config section; reuses `commit.model` / `commit.language`.
- **No** plugin hook — built-in subcommand, per ADR-0001.
- **No** direct-argv mode — `run` is always `sh -c` (see rationale above).
