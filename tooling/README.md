# Tooling

The parts of this project's Claude Code setup that live outside the repo and are
not recoverable from anywhere else.

## What is here

| file | what it is | why it is versioned |
|---|---|---|
| `standing-directives.md` | copy of `~/.claude/standing-directives.md` | loaded into every session by a `SessionStart` hook. Holds the measurement discipline, the OKF rules and the critical-advisor persona. Exists on one disk otherwise. |
| `claude-settings.json` | copy of `~/.claude/settings.json` | names the plugins and marketplaces, and defines the hook above. |

`memory/` is versioned too, at the repo root. It is the knowledge vault: every
measured number, every wrong turn and the reasoning behind decisions the code
cannot explain by itself. It was gitignored until 2026-09-09 on the grounds that
it was scratch notes; by then it had become the only record of *why* the engine
is built the way it is.

## What is deliberately not here

- **`~/.claude/.credentials.json`** — an OAuth token. This repository is public.
  A committed token is compromised permanently, because history survives
  deletion.
- **`~/.claude/projects/`** — 192 MB of conversation transcripts.
- **`~/.claude/plugins/`** — third-party plugin checkouts. Vendoring someone
  else's source is not backup; `claude-settings.json` names the marketplaces they
  come from, which is what actually needs restoring.

## Restoring on a new machine

1. Install the plugins named in `claude-settings.json` from the marketplaces it
   lists.
2. Copy `standing-directives.md` to `~/.claude/`.
3. Copy the `hooks` block from `claude-settings.json` into `~/.claude/settings.json`.

**The hook's path is absolute and Windows-specific**
(`C:\Users\Flori\.claude\standing-directives.md`). It needs editing anywhere
else, and nothing warns if it silently fails — a session simply starts without
the directives.

`critical-advisor` is worth calling out: it is an Anthropic skill, not a plugin,
so nothing in `enabledPlugins` restores it. It is active only because
`standing-directives.md` restates it and the hook loads that file.
