import { create } from "zustand";
import type { Channel, Member, MemberType, Message, Store } from "@agentchat/shared";
import { MockStore } from "./mock-store";

export const store: Store = new MockStore();

interface ChatState {
  channels: Channel[];
  members: Member[];
  messages: Message[];
  activeChannelId: string | null;
  inviteOpen: boolean;
  caddyOpen: boolean;
  prefsOpen: boolean;
  /** text the command palette wants inserted into the composer */
  composerInsert: string | null;
  /** agents currently composing, per channel */
  working: { channelId: string; memberId: string }[];
  init(): Promise<void>;
  selectChannel(channelId: string): Promise<void>;
  send(text: string): Promise<void>;
  resolveAsk(messageId: string, option: string): Promise<void>;
  setInviteOpen(open: boolean): void;
  setCaddyOpen(open: boolean): void;
  setPrefsOpen(open: boolean): void;
  setComposerInsert(text: string | null): void;
}

export const useChat = create<ChatState>((set, get) => ({
  channels: [],
  members: [],
  messages: [],
  activeChannelId: null,
  inviteOpen: false,
  caddyOpen: false,
  prefsOpen: false,
  composerInsert: null,
  working: [],

  async init() {
    // StrictMode double-invokes effects; never subscribe twice
    if (get().channels.length) return;
    const [channels, members] = await Promise.all([
      store.listChannels(),
      store.listMembers(),
    ]);
    set({ channels, members });
    store.subscribe((event) => {
      const { activeChannelId, messages } = get();
      if (event.type === "message" && event.message.channelId === activeChannelId) {
        if (!messages.some((m) => m.id === event.message.id)) {
          set({ messages: [...messages, event.message] });
        }
      }
      if (event.type === "message.updated") {
        set({
          messages: get().messages.map((m) =>
            m.id === event.message.id ? { ...event.message } : m,
          ),
        });
      }
      if (event.type === "working") {
        const rest = get().working.filter(
          (w) =>
            !(w.channelId === event.channelId && w.memberId === event.memberId),
        );
        set({
          working: event.working
            ? [...rest, { channelId: event.channelId, memberId: event.memberId }]
            : rest,
        });
      }
      if (event.type === "presence") {
        set({
          members: get().members.map((m) =>
            m.id === event.memberId ? { ...m, presence: event.presence } : m,
          ),
        });
      }
    });
    const first = channels[0];
    if (first) await get().selectChannel(first.id);
  },

  async selectChannel(channelId) {
    set({ activeChannelId: channelId });
    set({ messages: await store.listMessages(channelId) });
  },

  async send(text) {
    const { activeChannelId } = get();
    if (!activeChannelId || !text.trim()) return;
    await store.sendMessage(activeChannelId, text.trim());
  },

  async resolveAsk(messageId, option) {
    await store.resolveAsk(messageId, option);
  },

  setInviteOpen(open) {
    set({ inviteOpen: open });
  },

  setCaddyOpen(open) {
    set({ caddyOpen: open });
  },

  setPrefsOpen(open) {
    set({ prefsOpen: open });
  },

  setComposerInsert(text) {
    set({ composerInsert: text });
  },
}));

export function memberById(members: Member[], id: string): Member | undefined {
  return members.find((m) => m.id === id);
}

export async function createInvite(memberType: MemberType) {
  return store.createInvite(memberType);
}
