# Findings

Living doc of product insights discovered while building. Newest last.

## 2026-08-01 — Day one

### The four pillars
1. **Beautiful design (UI + UX)** — the only moat left in the AI agent era.
2. **AI-native** — agents are first-class citizens: same invite flow, presence, and member list as humans. Not bolted-on bots.
3. **Agent-agnostic** — any model, any CLI (Claude, Codex, Kimi, OpenClaw). Every new agent launch is distribution, not competition.
4. **First-class CLI** — the CLI can do everything the app can. The CLI is the protocol: any agent that can run a shell command is integrated.

### IA: WhatsApp, not Slack
Started with Slack-shaped channels; pivoted to the messenger model — one flat,
drag-sortable list of conversations: **DMs with agents** (agent always replies,
no @mention needed) and **created groups** (mention-triggered). Simpler mental
model, and it's what the target market already does (OpenClaw users live in
Telegram). Channels can return later as dressed-up groups for teams.

### Agentception: the app is agent-controllable
Every UI action routes through one command bus (`commands.ts`), so an in-app
brain can drive the app itself — ⌘K one-shot commands, ⌘J "caddy" for
multi-step plans. The command registry's schemas double as LLM tool
definitions, so any new command is automatically speakable. Agent actions
visibly "press" the UI (touch highlight + beat) — trust through visibility.

### THE finding: agents are shareable — cross-user agent chat
You can add **your friend's agent** to a group chat. This changes everything:

- **Viral loop**: sharing an agent drags in its owner. "Add your agent to the
  group" is an invite mechanic no chat app has had.
- **Agents become social objects**: something you tune, show off, and lend.
  Status mechanics driving infrastructure adoption.
- **Safety falls out of the existing `ask` primitive**: a lent agent's
  approvals route to its *owner's* phone, not the group. Owner keeps control,
  host stays safe, audit trail records both.
- **Architecture implication**: agent identity must be global from day one —
  name + owner + token, portable across workspaces. Design the relay (phase 2)
  around this; retrofitting federation is pain.
- **The launch demo**: two humans + both their agents in one group; human A
  asks human B's agent for something; the approval pops on B's phone.

### Design language findings
- Stock shadcn dark palette beats custom charcoal (lighter sidebars against a
  deeper background reads less "black slab").
- Monochrome dither-kit pixel avatars = the identity system. Unique pattern
  per name, one gray tone, hairline hue-free frame. Personality without color
  noise; matches the dithered chart/primitive aesthetic.
- Density: 85% root font, -0.011em tracking, 38px integrated titlebar,
  status bar with live shortcut hints. Terminal-adjacent, not terminal-cosplay.
- The 9 ported AI primitives (thinking trace, tool chips, streaming text,
  approval flows, task rows, code block, insights, recommendation) read as
  native *message types*, not embeds — this is what "AI-native chat" looks
  like concretely.

### Naming (parked, marinating)
Frontrunner: **Rebeam** (rebeam.app secured). Bench: Resonar, Recrew, Relay,
Remuse. Constraint: starts with "Re". Working name in code: `agentchat`.

### Licensing (decided direction, not yet applied)
FSL (Sentry's Functional Source License) — source-available, self-hosting
fine, competing commercially prohibited, converts to Apache after 2 years.
Decide formally before any external contributions arrive.

### Status protocol: how agents report thinking/actions (designed, not yet built)
Three tiers feeding one event stream:
1. **Bridge parsing** — `agentchat up` adapters translate each CLI's native
   structured output (e.g. `claude -p --output-format stream-json`) into relay
   events. Zero agent effort.
2. **Explicit verbs** — `agentchat status thinking|tool|progress|done` for
   anything that can shell. Bridge auto-emits working:true/false around runs.
3. **Rich streams** — `agentchat stream` (JSONL on stdin) and `agentchat mcp`
   (post_status/send_message/ask as MCP tools).

Unifying concept: the **turn** — turn.start → status/tool events append →
final text → turn.end. One evolving message in the UI: working loader →
ThinkingState → ToolChips → text, collapsing to "Thought for Ns ›".
The showcase primitives are the renderer for this wire format.

**CLI vs MCP decision:** CLI is the base protocol — zero config, universal
(anything that can shell). `agentchat mcp` is a thin optional wrapper over the
same command bus for MCP-native agents. Never require MCP.

### Hermes Agent (Nous Research) teardown — validation from the incumbent
Studied hermes-agent (the VPS-agent-in-Telegram archetype our users duct-tape):
- Their agent fires `tool.started`/`tool.completed`/`_thinking` callbacks with
  tool_name/preview/args/duration — same vocabulary as our turn events.
- Their gateway DEGRADES this telemetry into borrowed platforms: Slack status
  line ("is running pytest…"), Telegram typing bubbles, progress via message
  edits, per-chat verbosity modes (off/log/all, /verbose). Hundreds of lines
  of per-platform capability fallbacks (`supports_status_text`,
  `supports_code_blocks`).
- **Strategic read: their gateway complexity is the tax our native client
  eliminates.** We render the same events as first-class UI (ThinkingState,
  ToolChips) instead of squeezing them through typing indicators.
- Steal: status-phrase builder from tool+args, per-chat verbosity setting,
  duration on tool.completed, pausing the working indicator during approval
  waits, async wake-after-turn-ends for background process completions.

### Auto-updates (phase 4, decided)
tauri-plugin-updater + signed manifests on GitHub Releases via tauri-action CI.
Background download, "update ready — restart" chip in the status bar,
auto-apply on quit. Frontend-only changes = small silent updates. CLI updates
separately (`agentchat upgrade` / npm / brew); relay can nudge stale bridges.
Do NOT remote-load the webview as a shortcut.

### Two integration modes — bridge vs mirror (2026-08-03)
A Claude plugin is a great *on-ramp*, never the foundation: pillar 3 dies if
only one agent works. The CLI stays the protocol. But thinking it through
surfaced a second product mode we hadn't named:

- **`rebeam up` — bridge.** rebeam owns the agent, invoking it headless on
  each message. For unattended VPS boxes. *"My agent lives here."*
- **plugin / hooks — mirror.** The agent runs where the user already runs it,
  interactively; rebeam watches and mirrors. *"My laptop session is visible
  from my phone."*

Mirror is far cheaper to build and solves the gap hit while dogfooding: Claude
Code hooks (`PostToolUse` → `rebeam status tool`, `Stop` → `rebeam send`) emit
the whole turn stream with ~20 lines of config and no bridge process. It also
ships as an installable plugin into the largest agent population.

Build order: mirror hooks → `up` → both on the same CLI.
