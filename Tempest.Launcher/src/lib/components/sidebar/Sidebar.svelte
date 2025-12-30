<script lang="ts">
	import { House, Compass, Library, Plus, Settings, User, Box } from "@lucide/svelte";
	import SidebarItem from "./SidebarItem.svelte";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import { instanceMap } from "$lib/stores/instance";
	import { page } from "$app/state";

	let isModalOpen = $state(false);

	const navigation = [
		{ href: "/", icon: House, label: "Home" },
		{ href: "/explore", icon: Compass, label: "Explore" },
		{ href: "/library", icon: Library, label: "Library" },
	];
</script>

<aside class="flex h-screen w-16 flex-none flex-col items-center bg-base-200 py-4">
	<!-- Main Navigation -->
	<nav class="flex flex-col gap-2">
		<SidebarItem href="/" icon={House} label="Home" />
		<SidebarItem href="/explore" icon={Compass} label="Explore" />
		<SidebarItem href="/library" icon={Library} label="Library" />
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<!-- Instances -->
	<div class="flex flex-1 flex-col gap-2 overflow-y-auto overflow-x-hidden px-2 scrollbar-none">
		{#each Object.values($instanceMap) as instance}
			<SidebarItem
				icon={Box}
				label={instance.label}
				active={page.route.id == "/instance/[id]" && page.params.id == instance.id}
				href={`/instance/${instance.id}`}
			/>
		{/each}

		<button class="btn btn-ghost btn-square outline-none" onclick={() => (isModalOpen = true)}>
			<Plus size={20} />
		</button>
	</div>

	<!-- Bottom Actions -->
	<div class="mt-auto flex flex-col gap-2">
		<SidebarItem href="/settings" icon={Settings} label="Settings" />
		<SidebarItem href="/profile" icon={User} label="Profile" />
	</div>

	<InstanceWizard bind:open={isModalOpen} />
</aside>
