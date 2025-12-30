<script lang="ts">
	import { House, Compass, Library, Plus, Settings, User, Box } from "@lucide/svelte";
	import SidebarItem from "./SidebarItem.svelte";

	const navigation = [
		{ href: "/", icon: House, label: "Home" },
		{ href: "/explore", icon: Compass, label: "Explore" },
		{ href: "/library", icon: Library, label: "Library" },
	];

	const instances = [
		{ href: "/instance/ob57", icon: Box, label: "OB 57", circle: true },
		{ href: "/instance/ob58", icon: Box, label: "OB 58", circle: true },
	];
</script>

<aside class="flex h-screen w-16 flex-none flex-col items-center bg-base-200 py-4">
	<!-- Main Navigation -->
	<nav class="flex flex-col gap-2">
		{#each navigation as item}
			<SidebarItem {...item} />
		{/each}
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<!-- Instances -->
	<div class="flex flex-1 flex-col gap-2 overflow-y-auto overflow-x-hidden px-2 scrollbar-none">
		{#each instances as instance}
			<SidebarItem {...instance} />
		{/each}

		<button class="btn btn-ghost btn-square rounded-xl">
			<Plus size={20} />
		</button>
	</div>

	<!-- Bottom Actions -->
	<div class="mt-auto flex flex-col gap-2">
		<SidebarItem href="/settings" icon={Settings} label="Settings" />
		<SidebarItem href="/profile" icon={User} label="Profile" />
	</div>
</aside>
