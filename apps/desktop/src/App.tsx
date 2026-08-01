import { useEffect } from "react";
import {
  SidebarInset,
  SidebarProvider,
  useSidebar,
} from "@/components/ui/sidebar";
import { SidebarLeft } from "@/components/sidebar-left";
import { SidebarRight } from "@/components/sidebar-right";
import { ChatView } from "@/components/chat/chat-view";
import { InviteModal } from "@/components/invite-modal";
import { PrefsModal } from "@/components/prefs-modal";
import { CaddyPanel } from "@/components/caddy-panel";
import { TitleBar } from "@/components/title-bar";
import { StatusBar } from "@/components/status-bar";
import { CommandPalette } from "@/components/command-palette";
import { useChat } from "@/lib/use-chat";

function Shortcuts() {
  const { toggleSidebar } = useSidebar();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (e.key >= "1" && e.key <= "9") {
        const { channels, selectChannel } = useChat.getState();
        const channel = channels[Number(e.key) - 1];
        if (channel) {
          e.preventDefault();
          void selectChannel(channel.id);
        }
        return;
      }
      if (e.key === "j") {
        e.preventDefault();
        const { caddyOpen, setCaddyOpen } = useChat.getState();
        setCaddyOpen(!caddyOpen);
        return;
      }
      if (e.key === ",") {
        e.preventDefault();
        const { prefsOpen, setPrefsOpen } = useChat.getState();
        setPrefsOpen(!prefsOpen);
        return;
      }
      if (e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
        return;
      }
      if (e.key === ".") {
        e.preventDefault();
        const { rightSidebarOpen, setRightSidebarOpen } = useChat.getState();
        setRightSidebarOpen(!rightSidebarOpen);
        return;
      }
      if (e.key === "i") {
        e.preventDefault();
        const { activityOpen, setActivityOpen } = useChat.getState();
        setActivityOpen(!activityOpen);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggleSidebar]);

  return null;
}

export default function App() {
  const init = useChat((s) => s.init);
  const rightSidebarOpen = useChat((s) => s.rightSidebarOpen);

  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => {
    // native webview context menu never shows; app menus own right-click
    const onContextMenu = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);

  return (
    <SidebarProvider>
      <Shortcuts />
      <TitleBar />
      <SidebarLeft />
      <SidebarInset className="h-svh min-h-0 overflow-hidden pb-7 pt-11">
        <ChatView />
      </SidebarInset>
      <div
        className={
          "shrink-0 overflow-hidden transition-[width] duration-200 ease-linear " +
          (rightSidebarOpen ? "w-(--sidebar-width)" : "w-0")
        }
      >
        <SidebarRight className="pb-7 pt-11" />
      </div>
      <StatusBar />
      <CommandPalette />
      <CaddyPanel />
      <PrefsModal />
      <InviteModal />
    </SidebarProvider>
  );
}
