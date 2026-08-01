export type MemberType = "human" | "agent";
export type Presence = "online" | "offline";

export interface Member {
  id: string;
  name: string;
  type: MemberType;
  presence: Presence;
  /** short capability blurb, mostly for agents */
  bio?: string;
}

export interface Channel {
  id: string;
  name: string;
  topic?: string;
}

export type MessageKind = "text" | "ask";

export interface Message {
  id: string;
  channelId: string;
  authorId: string;
  kind: MessageKind;
  text: string;
  /** ask messages only */
  options?: string[];
  /** set once a human taps a button */
  resolvedOption?: string;
  /** renders a named AI primitive instead of text (showcase/demo) */
  demo?: string;
  createdAt: number;
}

export interface Invite {
  code: string;
  memberType: MemberType;
  createdAt: number;
}

export type StoreEvent =
  | { type: "message"; message: Message }
  | { type: "message.updated"; message: Message }
  | { type: "presence"; memberId: string; presence: Presence }
  | { type: "working"; channelId: string; memberId: string; working: boolean };

/**
 * The only surface the desktop app talks to. Phase 1 backs it with an
 * in-memory mock; phase 2 swaps in the relay client without UI changes.
 */
export interface Store {
  currentUserId: string;
  listChannels(): Promise<Channel[]>;
  listMembers(): Promise<Member[]>;
  listMessages(channelId: string): Promise<Message[]>;
  sendMessage(channelId: string, text: string): Promise<Message>;
  resolveAsk(messageId: string, option: string): Promise<Message>;
  createInvite(memberType: MemberType): Promise<Invite>;
  subscribe(listener: (event: StoreEvent) => void): () => void;
}
