import { useEffect } from "react";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/app-sidebar";
import { ChatView } from "@/components/chat/chat-view";
import { InviteModal } from "@/components/invite-modal";
import { TitleBar } from "@/components/title-bar";
import { StatusBar } from "@/components/status-bar";
import { useChat } from "@/lib/use-chat";

export default function App() {
  const init = useChat((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!e.metaKey || e.key < "1" || e.key > "9") return;
      const { channels, selectChannel } = useChat.getState();
      const channel = channels[Number(e.key) - 1];
      if (channel) {
        e.preventDefault();
        void selectChannel(channel.id);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <SidebarProvider>
      <TitleBar />
      <AppSidebar />
      <SidebarInset className="min-h-0 pb-7 pt-12">
        <ChatView />
      </SidebarInset>
      <StatusBar />
      <InviteModal />
    </SidebarProvider>
  );
}
