import { Link } from '@tanstack/solid-router';
import packageJson from '../../../package.json';
import {
	Sidebar,
	SidebarContent,
	SidebarFooter,
	SidebarGroup,
	SidebarGroupLabel,
	SidebarHeader,
	SidebarMenu,
	SidebarMenuButton,
	SidebarMenuItem,
    SidebarMenuSub,
    SidebarRail,
} from '@repo/ui-solid/base/sidebar';
import { TooltipX } from '@repo/ui-solid/custom/tooltip';
import { openSettings } from './settings/settings';
import { ChevronRight, Folder, LayoutDashboard, Plus, Settings, SquarePlayIcon } from 'lucide-solid';
import { createQuery, useQuery, useQueryClient } from '@tanstack/solid-query';
import { GroupInfo } from '@repo/core/cmd/tasks/get_task';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@repo/ui-solid/base/collapsible';
import { For, Match, Show, Switch } from 'solid-js';
import { Separator } from '@repo/ui-solid/base/separator';
import { cn } from '@repo/shared/lib/utils';
import { ScrollArea } from '@repo/ui-solid/base/scroll-area';
import { client, fnrpc } from '#/integrations/fnrpc/client.ts';
import { useLiveQuery } from '@tanstack/solid-db';
import { taskGroupExpandCollection } from '#/feat/task_tree/sync.ts';
import { openCreateBatchDialog } from './CreateBatchDialog';

const getButtonPx = (depth: number) => ({
	'padding-left': `${(2 + 6 * depth) * 0.25}rem`,
	'padding-right': `${(2 + 6 * depth) * 0.25}rem`,
});
const TaskTree = (p: {items: GroupInfo[]}) => {
	const expandedQ = useLiveQuery((q) =>
    q.from({ t: taskGroupExpandCollection })
  );
	const isExpanded = (groupId: string) =>
    expandedQ()?.some(i => i.id === groupId) ?? false;
	const toggle = (groupId: string) => {
    if (isExpanded(groupId)) {
      taskGroupExpandCollection.delete(groupId);
    } else {
      taskGroupExpandCollection.insert({ id: groupId });
    }
  };

	return <For each={p.items}>{item => (<SidebarMenuItem class='h-fit '>
		<Collapsible
			class="group/collapsible [&[data-expanded]>button>svg:first-child]:rotate-90"
			open={isExpanded(item.group_id)}
  		onOpenChange={() => toggle(item.group_id)}
		>
			<CollapsibleTrigger as={SidebarMenuButton} class='gap-px rounded-none items-center'>
				<ChevronRight class="transition-transform" />
				<Folder />
				<span class='h-4 text-sm leading-4 pl-0.75'>{item.group_id}</span>
			</CollapsibleTrigger>
			<CollapsibleContent class="relative">
				<Separator
						orientation="vertical"
						class={cn(`absolute bg-accent-foreground z-1`)}
						style={{
							left: `${1}rem`, // 直接用 inline style，最可靠
						}}
					/>
				<SidebarMenuSub class="border-0 m-0 p-0 gap-0">
					{item.tasks.map(task => (<SidebarMenuButton class='rounded-none' style={getButtonPx(1)}
						as={Link}
						to={`/group/${item.group_id}/${task.id}`}
						activeProps={{
							class: "bg-accent/70!"
						}}
					>
						<SquarePlayIcon />
						<span class='h-4 text-sm leading-4'>{task.id}</span>
					</SidebarMenuButton>))}
				</SidebarMenuSub>
			</CollapsibleContent>
		</Collapsible>
	</SidebarMenuItem>)}
	</For>
}

export function AppSidebar() {
	const queryClient = useQueryClient();
	const groupListQ = createQuery(() => client.get_group_list.queryOptions(null))

	const handleCreateBatch = () => {
		openCreateBatchDialog(() => {
			queryClient.invalidateQueries({
				queryKey: client.get_group_list.queryKey(null),
			});
		});
	};

	return (
		<Sidebar>
			<SidebarHeader class="flex-row items-center justify-between">
				<TooltipX content={`Version ${packageJson.version}`}>
					<Link to="/">
						<h1 class="flex gap-1">
							<span>Local</span>
							<span class="text-muted-foreground">Dub</span>
						</h1>
					</Link>
				</TooltipX>
				<TooltipX content="创建任务">
					<SidebarMenuButton onClick={handleCreateBatch} class="size-8 p-0">
						<Plus class="size-4" />
					</SidebarMenuButton>
				</TooltipX>
			</SidebarHeader>
			<SidebarContent>
				<ScrollArea scrollbarSize={10}>

				<SidebarMenu class='gap-0 p-0'>
					<Switch>
						<Match when={groupListQ.isPending}>
							<span>Loading...</span>
						</Match>
						<Match when={groupListQ.isError}>
							<span>Error: {groupListQ.error?.message}</span>
						</Match>
						<Match when={groupListQ.isSuccess}>
							<TaskTree items={groupListQ.data??[]} />
						</Match>
					</Switch>
				</SidebarMenu>
				</ScrollArea>

					{/* <SidebarMenu>
						<SidebarMenuItem>
							<SidebarMenuButton as={Link} to="/">
								<LayoutDashboard /> Dashboard
							</SidebarMenuButton>
						</SidebarMenuItem>
					</SidebarMenu> */}
			</SidebarContent>
			<SidebarFooter>
				<SidebarMenuButton onClick={()=> openSettings()}><Settings /> Settings</SidebarMenuButton>
			</SidebarFooter>
			<SidebarRail size='sm' />
		</Sidebar>
	);
}
