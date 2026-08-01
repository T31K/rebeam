import * as React from "react";
import { AtSignIcon, BellOffIcon, CopyIcon, HashIcon, PlusIcon, UnplugIcon } from "lucide-react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { MemberAvatar, PresenceDot } from "@/components/member-avatar";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

export function AppSidebar(props: React.ComponentProps<typeof Sidebar>) {
  const {
    channels,
    members,
    activeChannelId,
    selectChannel,
    setInviteOpen,
    setComposerInsert,
    agentTouch,
  } = useChat();
  const humans = members.filter((m) => m.type === "human");
  const agents = members.filter((m) => m.type === "agent");

  return (
    <Sidebar {...props}>
      <SidebarHeader className="px-4 pb-0 pt-3">
        <span className="text-sm font-semibold tracking-tight">
          t31k's workspace
        </span>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Channels</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {channels.map((channel, i) => (
                <SidebarMenuItem key={channel.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton
                        isActive={channel.id === activeChannelId}
                        onClick={() => selectChannel(channel.id)}
                        className={cn(
                          agentTouch === `channel:${channel.id}` && "agent-touch",
                        )}
                      >
                        <HashIcon className="size-3.5 text-muted-foreground" />
                        <span>{channel.name}</span>
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-48">
                      <ContextMenuItem onSelect={() => void selectChannel(channel.id)}>
                        <HashIcon className="size-4" /> Open channel
                        <ContextMenuShortcut>⌘{i + 1}</ContextMenuShortcut>
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(`#${channel.name}`)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem disabled>
                        <BellOffIcon className="size-4" /> Mute channel
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Agents</SidebarGroupLabel>
          <SidebarGroupAction
            title="Invite an agent"
            onClick={() => setInviteOpen(true)}
          >
            <PlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            <SidebarMenu>
              {agents.map((agent) => (
                <SidebarMenuItem key={agent.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton
                        className={cn(
                          "h-9",
                          agentTouch === `member:${agent.id}` && "agent-touch",
                        )}
                      >
                        <MemberAvatar member={agent} className="size-6" />
                        <span
                          className={cn(
                            "flex-1 truncate",
                            agent.presence === "offline" &&
                              "text-muted-foreground",
                          )}
                        >
                          {agent.name}
                        </span>
                        <PresenceDot member={agent} />
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-52">
                      <ContextMenuItem
                        onSelect={() => setComposerInsert(`@${agent.name} `)}
                      >
                        <AtSignIcon className="size-4" /> Mention
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(agent.name)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem disabled variant="destructive">
                        <UnplugIcon className="size-4" /> Disconnect agent
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Humans</SidebarGroupLabel>
          <SidebarGroupAction
            title="Invite a human"
            onClick={() => setInviteOpen(true)}
          >
            <PlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            <SidebarMenu>
              {humans.map((human) => (
                <SidebarMenuItem key={human.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton className="h-9">
                        <MemberAvatar member={human} className="size-6" />
                        <span className="flex-1 truncate">{human.name}</span>
                        <PresenceDot member={human} />
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-52">
                      <ContextMenuItem
                        onSelect={() => setComposerInsert(`@${human.name} `)}
                      >
                        <AtSignIcon className="size-4" /> Mention
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(human.name)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              onClick={() => setInviteOpen(true)}
              className={cn(agentTouch === "invite" && "agent-touch")}
            >
              <PlusIcon className="size-4" />
              <span>Invite people or agents</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
