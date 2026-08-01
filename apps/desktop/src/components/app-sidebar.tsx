import * as React from "react";
import { HashIcon, PlusIcon, ZapIcon } from "lucide-react";

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
  const { channels, members, activeChannelId, selectChannel, setInviteOpen } =
    useChat();
  const humans = members.filter((m) => m.type === "human");
  const agents = members.filter((m) => m.type === "agent");

  return (
    <Sidebar variant="inset" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" className="pointer-events-none">
              <div className="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-primary text-sidebar-primary-foreground">
                <ZapIcon className="size-4" />
              </div>
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="truncate font-semibold tracking-tight">
                  agentchat
                </span>
                <span className="truncate text-xs text-muted-foreground">
                  t31k's workspace
                </span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Channels</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {channels.map((channel) => (
                <SidebarMenuItem key={channel.id}>
                  <SidebarMenuButton
                    isActive={channel.id === activeChannelId}
                    onClick={() => selectChannel(channel.id)}
                  >
                    <HashIcon className="size-3.5 text-muted-foreground" />
                    <span>{channel.name}</span>
                  </SidebarMenuButton>
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
                  <SidebarMenuButton className="h-9">
                    <MemberAvatar member={agent} className="size-6" />
                    <span
                      className={cn(
                        "flex-1 truncate",
                        agent.presence === "offline" && "text-muted-foreground",
                      )}
                    >
                      {agent.name}
                    </span>
                    <PresenceDot member={agent} />
                  </SidebarMenuButton>
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
                  <SidebarMenuButton className="h-9">
                    <MemberAvatar member={human} className="size-6" />
                    <span className="flex-1 truncate">{human.name}</span>
                    <PresenceDot member={human} />
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={() => setInviteOpen(true)}>
              <PlusIcon className="size-4" />
              <span>Invite people or agents</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
