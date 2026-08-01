import { useEffect, useState } from "react";
import { HashIcon, PlusIcon, AtSignIcon } from "lucide-react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command";
import { MemberAvatar } from "@/components/member-avatar";
import { useChat } from "@/lib/use-chat";

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const { channels, members, selectChannel, setInviteOpen, setComposerInsert } =
    useChat();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const run = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <Command>
      <CommandInput placeholder="Jump to a channel, mention someone, or run a command…" />
      <CommandList>
        <CommandEmpty>Nothing matches.</CommandEmpty>
        <CommandGroup heading="Channels">
          {channels.map((channel, i) => (
            <CommandItem
              key={channel.id}
              value={`channel ${channel.name}`}
              onSelect={() => run(() => void selectChannel(channel.id))}
            >
              <HashIcon className="size-4 text-muted-foreground" />
              <span>{channel.name}</span>
              {channel.topic && (
                <span className="truncate text-xs text-muted-foreground">
                  {channel.topic}
                </span>
              )}
              <CommandShortcut>⌘{i + 1}</CommandShortcut>
            </CommandItem>
          ))}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Mention">
          {members.map((member) => (
            <CommandItem
              key={member.id}
              value={`mention ${member.name} ${member.type}`}
              onSelect={() =>
                run(() => setComposerInsert(`@${member.name} `))
              }
            >
              <AtSignIcon className="size-4 text-muted-foreground" />
              <MemberAvatar member={member} className="size-5" />
              <span>{member.name}</span>
              <span className="ml-auto text-[10px] uppercase text-muted-foreground">
                {member.type}
              </span>
            </CommandItem>
          ))}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Workspace">
          <CommandItem
            value="invite agent human"
            onSelect={() => run(() => setInviteOpen(true))}
          >
            <PlusIcon className="size-4 text-muted-foreground" />
            <span>Invite people or agents</span>
          </CommandItem>
        </CommandGroup>
      </CommandList>
      </Command>
    </CommandDialog>
  );
}
