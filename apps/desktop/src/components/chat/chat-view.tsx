import { HashIcon } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import { MessageList } from "@/components/chat/message-list";
import { Composer } from "@/components/chat/composer";
import { LoadingState } from "@/components/ai";
import { useChat, memberById } from "@/lib/use-chat";

function WorkingIndicator() {
  const { working, activeChannelId, members } = useChat();
  const entry = working.find((w) => w.channelId === activeChannelId);
  if (!entry) return null;
  const member = memberById(members, entry.memberId);
  return (
    <div className="px-6 pb-1.5">
      <LoadingState label={`${member?.name ?? "agent"} is working`} />
    </div>
  );
}

export function ChatView() {
  const { channels, activeChannelId } = useChat();
  const channel = channels.find((c) => c.id === activeChannelId);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-10 shrink-0 items-center gap-2 border-b px-4">
        <HashIcon className="size-4 text-muted-foreground" />
        <span className="text-sm font-semibold">{channel?.name ?? "…"}</span>
        {channel?.topic && (
          <>
            <Separator orientation="vertical" className="mx-1 h-4" />
            <span className="truncate text-xs text-muted-foreground">
              {channel.topic}
            </span>
          </>
        )}
      </header>
      <MessageList />
      <WorkingIndicator />
      <Composer />
    </div>
  );
}
