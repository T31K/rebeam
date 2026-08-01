import type { Message } from "@agentchat/shared";
import { CheckIcon, CircleAlertIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

export function ApprovalCard({ message }: { message: Message }) {
  const resolveAsk = useChat((s) => s.resolveAsk);
  const resolved = message.resolvedOption != null;

  return (
    <div
      className={cn(
        "mt-1 max-w-md rounded-lg border p-3",
        resolved
          ? "border-border bg-muted/30"
          : "border-amber-500/30 bg-amber-500/5",
      )}
    >
      <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <CircleAlertIcon
          className={cn("size-3.5", !resolved && "text-amber-400")}
        />
        Approval requested
      </div>
      <p className="mt-2 text-sm font-medium">{message.text}</p>
      <div className="mt-3 flex gap-2">
        {resolved ? (
          <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <CheckIcon className="size-3.5 text-emerald-400" />
            You chose{" "}
            <span className="font-semibold text-foreground">
              {message.resolvedOption}
            </span>
          </div>
        ) : (
          message.options?.map((option, i) => (
            <Button
              key={option}
              size="sm"
              variant={i === 0 ? "default" : "outline"}
              onClick={() => resolveAsk(message.id, option)}
            >
              {option}
            </Button>
          ))
        )}
      </div>
    </div>
  );
}
