<script lang="ts">
	import { page } from "$app/state";
	import {
		Compass,
		Download,
		FlaskConical,
		House,
		Library,
		Plus,
		ScrollText,
		Server,
		Settings,
		SquareTerminal,
		Terminal,
		Users,
	} from "@lucide/svelte";
	import { lobbyHost } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap, preparedInstances } from "$lib/stores/instance.svelte";
	import { lobbyServerProcessesList } from "$lib/stores/processes.svelte";
	import { modDeveloper } from "$lib/stores/settings.svelte";
	import { commandsPageOpen, instanceWizardOpen } from "$lib/stores/ui.svelte";
	import SidebarInstanceItem from "./SidebarInstanceItem.svelte";
	import SidebarItem from "./SidebarItem.svelte";
</script>

<aside class="bg-base-300 flex h-screen w-16 flex-none flex-col items-center py-4">
	<nav class="flex flex-col gap-2">
		{#if Object.keys(instanceMap.value).length === 0}
			<SidebarItem href="/onboarding" label={m.sidebar_get_started()}
				><Compass size={20} /></SidebarItem
			>
		{/if}
		<SidebarItem href="/" label={m.sidebar_home()}><House size={20} /></SidebarItem>
		<SidebarItem href="/library" label={m.sidebar_library()}><Library size={20} /></SidebarItem>
		<SidebarItem href="/downloads" label={m.sidebar_downloads()}
			><Download size={20} /></SidebarItem
		>
		<SidebarItem href="/servers" label={m.sidebar_servers()}><Server size={20} /></SidebarItem>
		{#if modDeveloper.value}
			<SidebarItem href="/converter" label={m.converter_title()}
				><FlaskConical size={20} /></SidebarItem
			>
		{/if}
		{#if lobbyHost.value}
			<SidebarItem href="/lobby" label={m.sidebar_lobby()}><Users size={20} /></SidebarItem>
		{/if}
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div
		class="instance-list flex flex-1 scrollbar-none flex-col gap-2 overflow-x-visible overflow-y-auto px-2"
	>
		{#each preparedInstances.value as instance (instance.id)}
			<SidebarInstanceItem
				{instance}
				href={`/instance/${instance.id}`}
				active={page.route.id == "/instance/[id]" && page.params.id == instance.id}
			/>
		{/each}

		<button
			class="btn btn-ghost btn-square outline-none"
			onclick={() => (instanceWizardOpen.value = true)}
		>
			<Plus size={20} />
		</button>
	</div>
	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="mt-auto flex flex-col gap-2">
		{#if commandsPageOpen.value}
			<SidebarItem
				label={m.commands_page_title()}
				active={page.url.pathname === "/commands"}
				href="/commands"
			>
				<Terminal size={20} />
			</SidebarItem>
		{/if}
		{#each lobbyServerProcessesList.value as lobbyServer}
			<SidebarItem
				label={lobbyServer.createOptions.name}
				active={page.route.id == "/lobby-admin/[pid]" &&
					page.params.pid == String(lobbyServer.child.pid)}
				href={`/lobby-admin/${lobbyServer.child.pid}`}
			>
				<SquareTerminal size={20} />
			</SidebarItem>
		{/each}
		<SidebarItem href="/logs" label={m.sidebar_logs()}><ScrollText size={20} /></SidebarItem>
		<SidebarItem href="/settings" label={m.sidebar_settings()}
			><Settings size={20} /></SidebarItem
		>
	</div>
</aside>
