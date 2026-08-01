import { useEffect } from "react";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/app-sidebar";
import { ChatView } from "@/components/chat/chat-view";
import { InviteModal } from "@/components/invite-modal";
import { useChat } from "@/lib/use-chat";

export default function App() {
  const init = useChat((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);

  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset className="min-h-0">
        <ChatView />
      </SidebarInset>
      <InviteModal />
    </SidebarProvider>
  );
}
