<script lang="ts">
	import { House, Compass, Library, Plus, Settings, User, Box } from "@lucide/svelte";
	import SidebarItem from "./SidebarItem.svelte";
	import { instanceMap, lastLaunchedInstanceId } from "$lib/stores/instance";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import { page } from "$app/state";
</script>

<aside class="flex h-screen w-16 flex-none flex-col items-center bg-base-300 py-4">
	<nav class="flex flex-col gap-2">
		<SidebarItem href="/" icon={House} label="Home" />
		<SidebarItem href="/library" icon={Library} label="Library" />
		<SidebarItem href="/character-select" icon={Compass} label="Champions" />
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="flex flex-1 flex-col gap-2 overflow-y-auto overflow-x-visible px-2 scrollbar-none">
		{#each Object.values($instanceMap) as instance}
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

	<div class="mt-auto flex flex-col gap-2">
		<SidebarItem href="/settings" icon={Settings} label="Settings" />
	</div>
</aside>
