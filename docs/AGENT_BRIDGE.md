# Agent bridge — ACP

> Status: **built and working** (2026-08-08). The bridge that runs a real agent
> behind a chat, via the Agent Client Protocol (ACP). Supersedes the earlier
> "shell `claude -p` and parse the final blob" idea.

## Why ACP

The bridge must be **agent-agnostic** (pillar 3) — any coding agent, one code
path. Instead of a bespoke parser + permission hack per provider, we speak
**ACP** (Zed's Agent Client Protocol): one client drives Claude, Codex, Gemini,
Cursor, Cline, … over JSON-RPC/stdio, getting streaming tool events and
permission requests for free.

- We depend on the **official Rust SDK**: `agent-client-protocol = "2.0.0"`
  (repo `agentclientprotocol/rust-sdk`). Buzz (block/buzz) hand-rolls its own
  ACP wire; we don't — less code, less drift.
- ACP is coding-agent-flavoured. Non-ACP agents (Hermes, arbitrary CLIs) fall
  back to the old shell-exec path in `cli/src/up.rs`.

## The pieces

```
Tauri app ⇄ relay ⇄ gateway (up.rs) ⇄ rebeam acp (ACP client) ⇄ claude-code-acp (ACP agent) ⇄ claude -p
```

- **`cli/src/acp.rs`** — the ACP **client**. Spawns an ACP agent over stdio,
  runs one prompt, translates `session/update` → text + tool-call telemetry and
  `session/request_permission` → a decision. Two modes:
  - `Terminal` — the interactive `rebeam acp` probe (prints to the terminal).
  - `Gateway` — driven by the gateway: posts a `Cmd::Status` per tool call to
    the relay (live chips in the app) and prints the final reply to stdout for
    the gateway to `Cmd::Send`.
  - Also the **adapter resolver** (`adapter_command`): maps `--provider` →
    a launcher (see below).
- **`cli/src/bin/claude-code-acp.rs`** — our own ACP **agent** for Claude. Wraps
  `claude -p --output-format stream-json`. **No Node, no API key** — it strips
  `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` so `claude` authenticates via the
  **Pro/Max subscription login**. Streams text + tool calls as ACP session
  updates; persists Claude's `session_id` per chat for continuity. Design ported
  from `harukitosa/claude-code-acp` (MIT).

## Provider resolution (`rebeam acp --provider …`)

| provider | resolves to | auth |
|---|---|---|
| `claude` (default) | our `claude-code-acp` binary (co-located next to `rebeam`) | **subscription** login, no Node |
| `claude-api` | `npx -y @agentclientprotocol/claude-agent-acp` (Agent SDK) | **API key** only (Anthropic blocks subscription for the SDK) |
| `codex` | `npx -y @zed-industries/codex-acp` | codex CLI login |
| `gemini` | `gemini --experimental-acp` (native) | gemini CLI login |
| `--command "…"` | verbatim override | — |

**Key auth fact:** the official `claude-agent-acp` adapter wraps the Claude
**Agent SDK**, which requires an API key (Anthropic policy blocks subscription
auth for the SDK). The only way to use the **subscription** is to wrap the
**CLI** (`claude -p`) — which is what `claude-code-acp` does. Buzz's Claude path
is API-key-only; ours isn't.

## How it plugs into the gateway (no `up.rs` surgery)

The gateway already runs a shell `exec` per message, captures stdout as the
reply (→ `Cmd::Send`), and sets `REBEAM_CONTEXT`/`REBEAM_CHAT`/`REBEAM_RELAY`/
`REBEAM_AS` env. So `rebeam connect --provider claude` records:

```
exec = "rebeam acp --provider claude --gateway --cwd '<project dir>'"
```

`rebeam acp --gateway` reads the prompt from `$REBEAM_CONTEXT`, the machine
token from `~/.rebeam/machine-token`, posts tool `Status` events to the relay,
and prints the reply on stdout for the gateway to `Send`. Tool chips + replies
appear live in the app.

## Working directory / project binding

`rebeam connect --cwd <dir>` (default: the directory connect is run from) →
baked into the exec → passed as ACP `NewSessionRequest.cwd` → `claude -p` runs
**inside the project**, reads its `CLAUDE.md`/docs, like `cd project && claude`.
Buzz has **no** per-agent working dir (agents bind to channels only) — this is
our differentiator. Run `rebeam connect` from the project dir, or pass `--cwd`.

## Session continuity (the "it remembers now" fix)

The gateway spawns the adapter **fresh per message**, so in-memory state can't
carry a conversation. Fix: `claude-code-acp` persists Claude's `session_id` per
chat at `~/.rebeam/sessions/<chat>.txt` (keyed by `$REBEAM_CHAT`) and
`--resume`s it on the next message. Verified: two separate processes, the second
remembers the first. Continuity kicks in from the **2nd message** in a chat
onward (the first creates the session).

> Buzz keeps a **long-lived process** with a `channel_id → session_id` map and
> reuses the session in-memory (no disk). That's the more scalable model (avoids
> respawn/model-reinit cost) and is the upgrade path if per-message spawn latency
> hurts. Our disk-persist approach is simpler and, unlike Buzz, **survives a
> process crash**.

## Permission / tools

Headless `claude -p` has no one to approve tools, so a gated tool dead-ends
("please approve"). Current stopgap: **`--dangerously-skip-permissions`** so the
agent runs its tools — fine for *your own agent on your own machine*, NOT for
lent/shared agents.

- Buzz **refuses** an unattended full-bypass (there's a test enforcing it). They
  use ACP permission **modes** (`default`/`acceptEdits`/`dontAsk`/`plan`) + the
  agent's own sandbox. But `acceptEdits` still gates Bash → wouldn't run a shell
  command headless, which is why Buzz's default runtime is Goose, not Claude.
- **The real fix / the moat:** the **approval card**. Don't skip permissions —
  let the agent hit a real `session/request_permission`, route it to the human
  ("claude-1 wants to run `node …` — Approve?"), resume on the tap. For a
  **lent** agent, the approval routes to the *owner's* phone (the cross-user
  sharing safety story). This is exactly the gate Buzz **couldn't** build (their
  workflow-approval is a stub). Plumbing to reuse: ACP `request_permission`
  `options[]` (`allow_once`/`allow_always`/`reject_once`/`reject_always`) +
  Buzz's `WorkflowApprovalCard.tsx` UX (token-keyed, note field, self-expiring).

`--strict-mcp-config` is also passed to the adapter, so the agent does **not**
inherit the user's personal MCP servers (reminders/calendar/contacts) — those
trigger macOS TCC prompts attributed to `rebeam`.

## Gotchas discovered

- **Apple Silicon code-signing.** `cp`-ing a new binary *over* an existing one
  invalidates its ad-hoc signature → macOS SIGKILLs it on launch (`zsh: killed`,
  exit 137). After any manual reinstall: `codesign --force --sign - <binary>`.
  The real `install.sh`/`cargo install` path builds fresh binaries and avoids
  this.
- **macOS TCC prompts** ("rebeam wants your reminders") come from the user's
  Claude MCP servers, not rebeam — fixed with `--strict-mcp-config`.
- **Seed collision:** the relay seeds a demo agent named `claude-main`; connect
  with a different name to avoid `resolve_member` ambiguity in a fresh DB.

## Open items / next

1. **Approval card** (the moat) — `request_permission` → `rebeam ask` → app card
   → resume. Per-agent policy: `trusted` (auto-allow, today's behaviour) vs
   `ask` (route to owner). Turn off blanket skip-permissions.
2. **Long-lived adapter + session reuse** (Buzz's pool model) if spawn latency
   or model re-init becomes a bottleneck.
3. **Persona file** (`.persona.md`: frontmatter + markdown-as-system-prompt,
   `runtime`/`model`/`triggers`) instead of piling flags onto `connect`.
4. **Codex/Gemini** adapters via the resolver (already wired; untested).

## CLI reference

```
rebeam acp --provider claude -m "hi"                 # interactive probe
rebeam acp --provider claude --dry-run -m x          # print resolved adapter, don't run
rebeam acp --provider claude --gateway --cwd <dir>   # gateway mode (what connect records)
rebeam connect <invite> --provider claude --name <n> [--cwd <dir>] [--no-gateway]
```
