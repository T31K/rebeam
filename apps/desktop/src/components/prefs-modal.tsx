import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { useChat } from "@/lib/use-chat";

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <p className="text-sm font-medium">{label}</p>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </div>
  );
}

export function PrefsModal() {
  const { prefsOpen, setPrefsOpen } = useChat();

  return (
    <Dialog open={prefsOpen} onOpenChange={setPrefsOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Preferences</DialogTitle>
          <DialogDescription>Workspace and app settings.</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col">
          <Row label="Workspace name">
            <Input defaultValue="t31k's workspace" className="h-8 w-44 text-sm" />
          </Row>
          <Separator />
          <Row label="Theme" hint="Light theme lands with the design pass">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              dark
            </span>
          </Row>
          <Separator />
          <Row label="Relay" hint="Hosted relay arrives in phase 2">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              mock
            </span>
          </Row>
          <Separator />
          <Row label="Notifications" hint="Push requires the hosted relay">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              off
            </span>
          </Row>
        </div>
      </DialogContent>
    </Dialog>
  );
}
