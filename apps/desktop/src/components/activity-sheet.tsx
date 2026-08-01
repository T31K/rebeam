import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { MemberAvatar, PresenceDot } from "@/components/member-avatar";
import { LoadingState } from "@/components/ai";
import { useChat } from "@/lib/use-chat";

export function ActivitySheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { members, channels, working } = useChat();
  const agents = members.filter((m) => m.type === "agent");

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="bottom" className="pb-8">
        <SheetHeader>
          <SheetTitle className="text-sm">Activity</SheetTitle>
          <SheetDescription className="text-xs">
            Relay connection and what your agents are up to.
          </SheetDescription>
        </SheetHeader>
        <div className="grid gap-4 px-4 sm:grid-cols-2">
          <div className="rounded-lg border p-3">
            <p className="font-mono text-[11px] uppercase tracking-wide text-muted-foreground">
              Relay
            </p>
            <p className="mt-1.5 flex items-center gap-1.5 text-sm">
              <span className="size-1.5 rounded-full bg-emerald-400" />
              connected · mock (in-memory)
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {channels.length} channels · {members.length} members
            </p>
          </div>
          <div className="rounded-lg border p-3">
            <p className="font-mono text-[11px] uppercase tracking-wide text-muted-foreground">
              Agents
            </p>
            <div className="mt-1.5 flex flex-col gap-1.5">
              {agents.map((agent) => {
                const busy = working.find((w) => w.memberId === agent.id);
                return (
                  <div key={agent.id} className="flex items-center gap-2">
                    <MemberAvatar member={agent} className="size-5" />
                    <span className="flex-1 truncate text-sm">{agent.name}</span>
                    {busy ? (
                      <LoadingState label="working" variant="Dots" />
                    ) : (
                      <PresenceDot member={agent} />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}
