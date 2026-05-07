<script lang="ts">
	import {
		Box,
		Compass,
		Download,
		House,
		Library,
		Plus,
		Server,
		Settings,
		SquareTerminal,
	} from "@lucide/svelte";
	import { page } from "$app/state";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap, lastLaunchedInstanceId } from "$lib/stores/instance";
	import { lobbyHost } from "$lib/stores/lobby";
	import { lobbyServerProcessesList } from "$lib/stores/processes";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import LanguageSelector from "./LanguageSelector.svelte";
	import SidebarItem from "./SidebarItem.svelte";
</script>

<aside class="flex h-screen w-16 flex-none flex-col items-center bg-base-300 py-4">
	<nav class="flex flex-col gap-2">
		<SidebarItem href="/" icon={House} label={m.sidebar_home()} />
		<SidebarItem href="/library" icon={Library} label={m.sidebar_library()} />
		<SidebarItem href="/downloads" icon={Download} label={m.sidebar_downloads()} />
		<SidebarItem href="/servers" icon={Server} label={m.sidebar_servers()} />
		{#if $lobbyHost}
			<SidebarItem href="/lobby" icon={Compass} label={m.sidebar_lobby()} />
		{/if}
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="flex flex-1 flex-col gap-2 overflow-y-auto overflow-x-visible px-2 scrollbar-none">
		{#each Object.values($instanceMap).filter((i) => i.state?.type === "prepared") as instance}
			<SidebarItem
				icon={Box}
				label={instance.label}
				active={page.route.id == "/instance/[id]" && page.params.id == instance.id}
				href={`/instance/${instance.id}`}
			/>
		{/each}

		<button
			class="btn btn-ghost btn-square outline-none"
			onclick={() => instanceWizardOpen.set(true)}
		>
			<Plus size={20} />
		</button>
	</div>
	<div class="flex flex-col gap-2 overflow-y-auto overflow-x-visible px-2 scrollbar-none">
		{#each $lobbyServerProcessesList as lobbyServer}
			<SidebarItem
				icon={SquareTerminal}
				label={lobbyServer.createOptions.name}
				active={page.route.id == "/lobby-admin/[pid]" &&
					page.params.pid == String(lobbyServer.child.pid)}
				href={`/lobby-admin/${lobbyServer.child.pid}`}
			/>
		{/each}
	</div>
	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="mt-auto flex flex-col gap-2">
		<LanguageSelector />
		<SidebarItem href="/settings" icon={Settings} label={m.sidebar_settings()} />
	</div>
</aside>
