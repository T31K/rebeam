import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import {
  MoreHorizontalIcon,
  LinkIcon,
  BellOffIcon,
  HashIcon,
  PlusIcon,
  MessageCircleIcon,
  WrenchIcon,
  FlaskConicalIcon,
  SparklesIcon,
  type LucideIcon,
} from "lucide-react"
import { useChat } from "@/lib/use-chat"
import { cn } from "@/lib/utils"

const CHANNEL_ICONS: Record<string, LucideIcon> = {
  main: MessageCircleIcon,
  dev: WrenchIcon,
  research: FlaskConicalIcon,
  showcase: SparklesIcon,
}

export function NavFavorites() {
  const { isMobile } = useSidebar()
  const { channels, activeChannelId, selectChannel, agentTouch, setInviteOpen } =
    useChat()

  return (
    <SidebarGroup className="pt-0 group-data-[collapsible=icon]:hidden">
      <SidebarGroupLabel>Chats</SidebarGroupLabel>
      <SidebarGroupAction title="New chat" onClick={() => setInviteOpen(true)}>
        <PlusIcon />
      </SidebarGroupAction>
      <SidebarMenu>
        {channels.map((channel) => (
          <SidebarMenuItem key={channel.id}>
            <SidebarMenuButton
              title={channel.topic ?? channel.name}
              isActive={channel.id === activeChannelId}
              onClick={() => void selectChannel(channel.id)}
              className={cn(
                agentTouch === `channel:${channel.id}` && "agent-touch",
              )}
            >
              {(() => {
                const Icon = CHANNEL_ICONS[channel.name] ?? HashIcon
                return <Icon className="size-4 text-muted-foreground" />
              })()}
              <span className="capitalize">{channel.name}</span>
            </SidebarMenuButton>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <SidebarMenuAction
                  showOnHover
                  className="aria-expanded:bg-muted"
                >
                  <MoreHorizontalIcon />
                  <span className="sr-only">More</span>
                </SidebarMenuAction>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                className="w-56 rounded-lg"
                side={isMobile ? "bottom" : "right"}
                align={isMobile ? "end" : "start"}
              >
                <DropdownMenuItem
                  onSelect={() => void selectChannel(channel.id)}
                >
                  <HashIcon className="text-muted-foreground" />
                  <span>Open channel</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() =>
                    void navigator.clipboard.writeText(`#${channel.name}`)
                  }
                >
                  <LinkIcon className="text-muted-foreground" />
                  <span>Copy name</span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem disabled>
                  <BellOffIcon className="text-muted-foreground" />
                  <span>Mute channel</span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        ))}
        <SidebarMenuItem>
          <SidebarMenuButton className="text-sidebar-foreground/70">
            <MoreHorizontalIcon />
            <span>More</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>
    </SidebarGroup>
  )
}
