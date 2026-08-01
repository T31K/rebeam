import type { Member } from "@agentchat/shared";
import { BotIcon } from "lucide-react";
import { cn } from "@/lib/utils";

const agentColors: Record<string, string> = {
  a_claude: "bg-orange-500/15 text-orange-400",
  a_kimi: "bg-sky-500/15 text-sky-400",
  a_codex: "bg-emerald-500/15 text-emerald-400",
};

export function MemberAvatar({
  member,
  className,
}: {
  member: Member;
  className?: string;
}) {
  if (member.type === "agent") {
    return (
      <div
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-md",
          agentColors[member.id] ?? "bg-violet-500/15 text-violet-400",
          className,
        )}
      >
        <BotIcon className="size-4" />
      </div>
    );
  }
  return (
    <div
      className={cn(
        "flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-semibold uppercase text-primary",
        className,
      )}
    >
      {member.name.slice(0, 2)}
    </div>
  );
}

export function PresenceDot({ member }: { member: Member }) {
  return (
    <span
      className={cn(
        "size-1.5 shrink-0 rounded-full",
        member.presence === "online"
          ? "bg-emerald-400"
          : "border border-muted-foreground/40 bg-transparent",
      )}
    />
  );
}
