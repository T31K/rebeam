import { HashIcon } from "lucide-react";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Separator } from "@/components/ui/separator";
import { MessageList } from "@/components/chat/message-list";
import { Composer } from "@/components/chat/composer";
import { useChat } from "@/lib/use-chat";

export function ChatView() {
  const { channels, activeChannelId, members } = useChat();
  const channel = channels.find((c) => c.id === activeChannelId);
  const online = members.filter(
    (m) => m.type === "agent" && m.presence === "online",
  ).length;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
        <SidebarTrigger className="-ml-1" />
        <Separator orientation="vertical" className="mr-1 h-4" />
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
        <span className="ml-auto text-[11px] text-muted-foreground">
          {online} agent{online === 1 ? "" : "s"} online
        </span>
      </header>
      <MessageList />
      <Composer />
    </div>
  );
}
