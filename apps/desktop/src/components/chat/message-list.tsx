import { useMemo } from "react";
import type { Member, Message } from "@agentchat/shared";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from "@/components/ui/message-scroller";
import { MemberAvatar } from "@/components/member-avatar";
import { ApprovalCard } from "@/components/chat/approval-card";
import { useChat, memberById } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const GROUP_WINDOW_MS = 5 * 60_000;

function formatTime(ts: number) {
  return new Date(ts).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
}

function renderText(text: string, members: Member[]) {
  const names = members.map((m) => m.name).join("|");
  if (!names) return text;
  const parts = text.split(new RegExp(`(@(?:${names}))`, "g"));
  return parts.map((part, i) =>
    part.startsWith("@") ? (
      <span
        key={i}
        className="rounded bg-primary/10 px-1 py-0.5 font-medium text-primary"
      >
        {part}
      </span>
    ) : (
      <span key={i}>{part}</span>
    ),
  );
}

export function MessageList() {
  const { messages, members } = useChat();

  const grouped = useMemo(() => {
    return messages.map((message, i) => {
      const prev = messages[i - 1];
      const continuation =
        prev != null &&
        prev.authorId === message.authorId &&
        prev.kind === "text" &&
        message.kind === "text" &&
        message.createdAt - prev.createdAt < GROUP_WINDOW_MS;
      return { message, continuation };
    });
  }, [messages]);

  return (
    <MessageScrollerProvider>
      <MessageScroller className="flex-1">
        <MessageScrollerViewport>
          <MessageScrollerContent className="gap-0 px-4 py-3">
            {grouped.map(({ message, continuation }) => (
              <MessageScrollerItem key={message.id}>
                <MessageRow
                  message={message}
                  members={members}
                  continuation={continuation}
                />
              </MessageScrollerItem>
            ))}
          </MessageScrollerContent>
        </MessageScrollerViewport>
        <MessageScrollerButton />
      </MessageScroller>
    </MessageScrollerProvider>
  );
}

function MessageRow({
  message,
  members,
  continuation,
}: {
  message: Message;
  members: Member[];
  continuation: boolean;
}) {
  const author = memberById(members, message.authorId);
  if (!author) return null;

  return (
    <div
      className={cn(
        "group flex gap-3 rounded-md px-2 py-0.5 hover:bg-muted/30",
        continuation ? "mt-0" : "mt-3",
      )}
    >
      <div className="w-7 shrink-0 pt-0.5">
        {!continuation && <MemberAvatar member={author} />}
      </div>
      <div className="min-w-0 flex-1">
        {!continuation && (
          <div className="flex items-baseline gap-2">
            <span className="text-sm font-semibold">{author.name}</span>
            {author.type === "agent" && (
              <span className="rounded border border-border px-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                agent
              </span>
            )}
            <span className="text-[11px] text-muted-foreground">
              {formatTime(message.createdAt)}
            </span>
          </div>
        )}
        {message.kind === "ask" ? (
          <ApprovalCard message={message} />
        ) : (
          <p className="text-sm leading-relaxed text-foreground/90">
            {renderText(message.text, members)}
          </p>
        )}
      </div>
    </div>
  );
}
