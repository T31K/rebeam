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
import { CopyIcon, AtSignIcon, CornerUpLeftIcon } from "@/components/ui/icons";
import { Badge } from "@/components/ui/badge";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { MemberAvatar } from "@/components/member-avatar";
import { ApprovalCard } from "@/components/chat/approval-card";
import { MessageText } from "@/components/chat/message-text";
import {
  LoadingState,
  ThinkingState,
  StreamingText,
  ApprovalFlow,
  ToolChips,
  TaskRows,
  CodeBlock,
  InsightCards,
  RecommendationCard,
  DitherChartDemo,
} from "@/components/ai";
import { useChat, memberById } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const DEMOS: Record<string, React.ReactNode> = {
  loading: <LoadingState label="Churning" />,
  thinking: <ThinkingState variant="Steps" />,
  streaming: <StreamingText />,
  "approval-flow": <ApprovalFlow />,
  "tool-chips": <ToolChips />,
  tasks: <TaskRows />,
  code: <CodeBlock />,
  insights: <InsightCards />,
  recommendation: <RecommendationCard />,
  "dither-chart": <DitherChartDemo />,
};

const GROUP_WINDOW_MS = 5 * 60_000;

function formatTime(ts: number) {
  return new Date(ts).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
}

function UnreadDivider({ count }: { count: number }) {
  return (
    <div className="my-2 flex items-center gap-2 px-2">
      <span className="h-px flex-1 bg-foreground/30" />
      <Badge variant="secondary" className="rounded-full text-[10px]">
        {count} unread message{count === 1 ? "" : "s"}
      </Badge>
      <span className="h-px flex-1 bg-foreground/30" />
    </div>
  );
}

export function MessageList() {
  const { messages, members, activeChannelId, unreadMarker } = useChat();

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
      <MessageScroller className="min-h-0 flex-1">
        <MessageScrollerViewport>
          {/* Extra room at the foot so the newest message never sits flush
              against the composer — the crowding reads as a cut-off edge. */}
          <MessageScrollerContent className="gap-0 px-4 pt-3 pb-6">
            {grouped.map(({ message, continuation }) => (
              <MessageScrollerItem key={message.id}>
                {unreadMarker?.channelId === activeChannelId &&
                  unreadMarker.messageId === message.id && (
                    <UnreadDivider count={unreadMarker.count} />
                  )}
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

/**
 * Just the `**bold**` spans, which is all a system line ever carries — the
 * member who joined or left. Cheaper and more predictable here than routing a
 * one-line label through the full markdown renderer.
 */
function SystemText({ text }: { text: string }) {
  return (
    <>
      {text.split(/\*\*(.+?)\*\*/g).map((part, i) =>
        i % 2 === 1 ? (
          <span key={i} className="font-medium text-foreground">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
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
  const setComposerInsert = useChat((s) => s.setComposerInsert);
  const author = memberById(members, message.authorId);
  if (!author) return null;

  // Membership changes are the room talking about itself, not somebody in it.
  // No avatar, no name column, no reply affordance — a rule across the thread.
  if (message.kind === "system") {
    return (
      <div className="my-2 flex items-center gap-2 px-2">
        <span className="h-px flex-1 bg-border" />
        <Badge variant="secondary" className="rounded-full text-[10px] font-normal">
          <SystemText text={`${author.name} ${message.text}`} />
        </Badge>
        <span className="h-px flex-1 bg-border" />
      </div>
    );
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          className={cn(
            // More room below than above: the highlight should close under
            // the last line rather than clipping it.
            "group flex gap-3 rounded-md px-2 pt-1 pb-2 hover:bg-muted/30",
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
        {message.demo && DEMOS[message.demo] ? (
          <div className="mt-1">
            {message.text && (
              <p className="mb-1.5 text-sm leading-relaxed text-foreground/90">
                {message.text}
              </p>
            )}
            {DEMOS[message.demo]}
          </div>
        ) : message.kind === "ask" ? (
          <ApprovalCard message={message} />
        ) : (
          <MessageText text={message.text} members={members} />
        )}
        </div>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-52">
        <ContextMenuItem onSelect={() => setComposerInsert(`@${author.name} `)}>
          <CornerUpLeftIcon className="size-4" /> Reply to {author.name}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => setComposerInsert(`@${author.name} `)}>
          <AtSignIcon className="size-4" /> Mention {author.name}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          onSelect={() => void navigator.clipboard.writeText(message.text)}
        >
          <CopyIcon className="size-4" /> Copy text
        </ContextMenuItem>
        <ContextMenuItem
          onSelect={() => void navigator.clipboard.writeText(message.id)}
        >
          <CopyIcon className="size-4" /> Copy message ID
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
