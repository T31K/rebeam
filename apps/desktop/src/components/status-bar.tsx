import { ActivitySheet } from "@/components/activity-sheet";
import { useChat } from "@/lib/use-chat";

function Key({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="rounded-sm border border-border bg-muted/40 px-1 py-px font-mono text-[10px] text-foreground/70">
      {children}
    </kbd>
  );
}

function Hint({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <Key>{keys}</Key>
      <span>{label}</span>
    </span>
  );
}

export function StatusBar() {
  const { channels, activeChannelId, activityOpen, setActivityOpen } = useChat();
  const idx = channels.findIndex((c) => c.id === activeChannelId);

  return (
    <footer className="fixed inset-x-0 bottom-0 z-30 flex h-7 items-center gap-4 border-t bg-sidebar px-3 font-mono text-[11px] text-muted-foreground">
      <button
        className="flex items-center gap-1.5 rounded-sm px-1 transition-colors hover:bg-muted/40 hover:text-foreground"
        onClick={() => setActivityOpen(true)}
      >
        <span className="size-1.5 rounded-full bg-emerald-400" />
        connected · mock relay
      </button>
      <ActivitySheet open={activityOpen} onOpenChange={setActivityOpen} />
      <div className="ml-auto flex items-center gap-4">
        <Hint keys="⌘K" label="agent" />
        <Hint keys="⌘J" label="caddy" />
        <Hint keys="⌘1–9" label="channels" />
        <Hint keys="⌘\" label="left" />
        <Hint keys="⌘." label="right" />
        <Hint keys="⌘," label="prefs" />
        {idx >= 0 && (
          <span className="text-foreground/60">
            #{channels[idx].name} [{idx + 1}/{channels.length}]
          </span>
        )}
      </div>
    </footer>
  );
}
