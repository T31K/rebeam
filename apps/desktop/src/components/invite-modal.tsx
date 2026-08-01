import { useState } from "react";
import type { Invite } from "@agentchat/shared";
import { BotIcon, CheckIcon, CopyIcon, UserIcon } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { createInvite, useChat } from "@/lib/use-chat";

export function InviteModal() {
  const { inviteOpen, setInviteOpen } = useChat();
  const [invite, setInvite] = useState<Invite | null>(null);
  const [copied, setCopied] = useState(false);

  const generate = async () => {
    setInvite(await createInvite("agent"));
    setCopied(false);
  };

  const snippet = invite
    ? `curl -fsSL https://agentchat.dev/install.sh | sh\nagentchat join ${invite.code}`
    : "";

  const copy = async () => {
    await navigator.clipboard.writeText(snippet);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Invite to workspace</DialogTitle>
          <DialogDescription>
            Humans and agents are both first-class citizens here.
          </DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="agent">
          <TabsList className="w-full">
            <TabsTrigger value="agent" className="flex-1 gap-1.5">
              <BotIcon className="size-3.5" /> Agent
            </TabsTrigger>
            <TabsTrigger value="human" className="flex-1 gap-1.5">
              <UserIcon className="size-3.5" /> Human
            </TabsTrigger>
          </TabsList>

          <TabsContent value="agent" className="space-y-3 pt-2">
            {invite ? (
              <>
                <p className="text-sm text-muted-foreground">
                  Run this wherever your agent lives — laptop, VPS, Mac mini:
                </p>
                <div className="relative rounded-lg border bg-muted/40 p-3 font-mono text-xs leading-relaxed">
                  <pre className="whitespace-pre-wrap">{snippet}</pre>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="absolute right-1.5 top-1.5 size-7"
                    onClick={copy}
                  >
                    {copied ? (
                      <CheckIcon className="size-3.5 text-emerald-400" />
                    ) : (
                      <CopyIcon className="size-3.5" />
                    )}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  The agent shows up in the sidebar with its own identity and
                  presence the moment it joins.
                </p>
              </>
            ) : (
              <Button className="w-full" onClick={generate}>
                Generate invite code
              </Button>
            )}
          </TabsContent>

          <TabsContent value="human" className="space-y-3 pt-2">
            <p className="text-sm text-muted-foreground">
              Send an invite link to a teammate:
            </p>
            <div className="flex gap-2">
              <Input placeholder="teammate@example.com" type="email" />
              <Button disabled title="Coming with the hosted relay">
                Send
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Email invites arrive with the hosted relay (phase 2).
            </p>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
