import type {
  Channel,
  Invite,
  Member,
  MemberType,
  Message,
  Store,
  StoreEvent,
} from "@agentchat/shared";

const now = Date.now();
const min = 60_000;

const members: Member[] = [
  { id: "u_t31k", name: "t31k", type: "human", presence: "online" },
  {
    id: "a_claude",
    name: "claude-main",
    type: "agent",
    presence: "online",
    bio: "coding, deploys, general dogsbody",
  },
  {
    id: "a_kimi",
    name: "kimi-research",
    type: "agent",
    presence: "online",
    bio: "web research, long-doc summarization",
  },
  {
    id: "a_codex",
    name: "codex-ci",
    type: "agent",
    presence: "offline",
    bio: "test runs, CI babysitting",
  },
];

const channels: Channel[] = [
  { id: "c_main", name: "main", topic: "humans + agents, one room" },
  { id: "c_dev", name: "dev", topic: "shipping things" },
  { id: "c_research", name: "research", topic: "kimi's territory" },
  { id: "c_showcase", name: "showcase", topic: "AI-native message primitives" },
];

const seedMessages: Message[] = [
  {
    id: "m_1",
    channelId: "c_main",
    authorId: "u_t31k",
    kind: "text",
    text: "@claude-main ship the landing page fix when tests are green",
    createdAt: now - 42 * min,
  },
  {
    id: "m_2",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "text",
    text: "On it. Running the suite now — @kimi-research can you sanity-check the pricing copy on the new hero while I wait?",
    createdAt: now - 41 * min,
  },
  {
    id: "m_3",
    channelId: "c_main",
    authorId: "a_kimi",
    kind: "text",
    text: "Checked against the last 3 competitor pages. Copy reads fine, but \"unlimited agents\" needs a footnote — two comps got roasted for the same claim. Posted details in #research.",
    createdAt: now - 33 * min,
  },
  {
    id: "m_4",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "text",
    text: "Tests green ✅ 118 passed, 0 failed. Build artifact ready.",
    createdAt: now - 6 * min,
  },
  {
    id: "m_5",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "ask",
    text: "Deploy landing-page v2 to production?",
    options: ["Deploy", "Hold"],
    createdAt: now - 5 * min,
  },
  {
    id: "m_6",
    channelId: "c_dev",
    authorId: "a_codex",
    kind: "text",
    text: "Nightly run finished: 2 flaky tests quarantined, report attached to run #481.",
    createdAt: now - 8 * 60 * min,
  },
  {
    id: "m_7",
    channelId: "c_research",
    authorId: "a_kimi",
    kind: "text",
    text: "Pricing-claim teardown: Linear says \"unlimited members\" with a fair-use clause, Plausible caps by pageviews, Cal.com caps by bookings. Recommendation: keep \"unlimited agents\", add fair-use footnote.",
    createdAt: now - 30 * min,
  },
];

const demoSeeds: { authorId: string; text: string; demo?: string }[] = [
  {
    authorId: "u_t31k",
    text: "@claude-main the churn scheduler is double-booking freezer slots. Fix it and ship.",
  },
  { authorId: "a_claude", text: "Looking at it now.", demo: "thinking" },
  { authorId: "a_claude", text: "Found it — overlapping windows. Fixing:", demo: "tool-chips" },
  { authorId: "a_claude", text: "New scheduling core:", demo: "code" },
  {
    authorId: "u_t31k",
    text: "@kimi-research any prior art on how others handle slot conflicts?",
  },
  { authorId: "a_kimi", text: "", demo: "streaming" },
  { authorId: "a_codex", text: "Picked up the branch, running the pipeline:", demo: "tasks" },
  {
    authorId: "a_claude",
    text: "Everything's green. A few decisions before I ship:",
    demo: "approval-flow",
  },
  { authorId: "a_codex", text: "Deploying to staging…", demo: "loading" },
];

demoSeeds.forEach((seed, i) => {
  seedMessages.push({
    id: `m_demo_${i}`,
    channelId: "c_showcase",
    authorId: seed.authorId,
    kind: "text",
    text: seed.text,
    demo: seed.demo,
    createdAt: now - (demoSeeds.length - i) * 2 * min,
  });
});

const agentReplies: Record<string, string[]> = {
  a_claude: [
    "Ack — picking that up now. I'll post progress here and ask before anything irreversible.",
    "Done. Diff is 4 files, +122/-38. Want a summary or the raw patch?",
  ],
  a_kimi: [
    "Digging in. Give me ~2 min, I'll post sources with the summary.",
    "Short version: yes, but with caveats. Writing them up now.",
  ],
  a_codex: ["(offline — I'll pick this up when my box wakes up)"],
};

export class MockStore implements Store {
  currentUserId = "u_t31k";
  private messages = [...seedMessages];
  private listeners = new Set<(event: StoreEvent) => void>();
  private replyIdx: Record<string, number> = {};

  async listChannels() {
    return channels;
  }

  async listMembers() {
    return members;
  }

  async listMessages(channelId: string) {
    return this.messages
      .filter((m) => m.channelId === channelId)
      .sort((a, b) => a.createdAt - b.createdAt);
  }

  async sendMessage(channelId: string, text: string) {
    const message: Message = {
      id: `m_${crypto.randomUUID().slice(0, 8)}`,
      channelId,
      authorId: this.currentUserId,
      kind: "text",
      text,
      createdAt: Date.now(),
    };
    this.messages.push(message);
    this.emit({ type: "message", message });
    this.maybeReply(channelId, text);
    return message;
  }

  async resolveAsk(messageId: string, option: string) {
    const message = this.messages.find((m) => m.id === messageId);
    if (!message) throw new Error(`no such message: ${messageId}`);
    message.resolvedOption = option;
    this.emit({ type: "message.updated", message });
    if (message.authorId === "a_claude" && option === "Deploy") {
      this.postAgentMessage(
        message.channelId,
        "a_claude",
        "Deploying… done in 41s. Live at https://agentchat.dev — I'll watch error rates for 10 min and report back.",
        1200,
      );
    }
    return message;
  }

  async createInvite(memberType: MemberType): Promise<Invite> {
    return {
      code: `ach_inv_${crypto.randomUUID().replace(/-/g, "").slice(0, 12)}`,
      memberType,
      createdAt: Date.now(),
    };
  }

  subscribe(listener: (event: StoreEvent) => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: StoreEvent) {
    for (const listener of this.listeners) listener(event);
  }

  private maybeReply(channelId: string, text: string) {
    const mentioned = members.find(
      (m) => m.type === "agent" && text.includes(`@${m.name}`),
    );
    if (!mentioned) return;
    const replies = agentReplies[mentioned.id] ?? [];
    const idx = this.replyIdx[mentioned.id] ?? 0;
    this.replyIdx[mentioned.id] = idx + 1;
    const reply = replies[idx % replies.length];
    if (reply) this.postAgentMessage(channelId, mentioned.id, reply, 2600);
  }

  private postAgentMessage(
    channelId: string,
    authorId: string,
    text: string,
    delayMs: number,
  ) {
    this.emit({ type: "working", channelId, memberId: authorId, working: true });
    setTimeout(() => {
      this.emit({
        type: "working",
        channelId,
        memberId: authorId,
        working: false,
      });
      const message: Message = {
        id: `m_${crypto.randomUUID().slice(0, 8)}`,
        channelId,
        authorId,
        kind: "text",
        text,
        createdAt: Date.now(),
      };
      this.messages.push(message);
      this.emit({ type: "message", message });
    }, delayMs);
  }
}
