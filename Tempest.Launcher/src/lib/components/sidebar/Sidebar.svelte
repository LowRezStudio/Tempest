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
		Users,
	} from "@lucide/svelte";
	import { flip } from "svelte/animate";
	import { lobbyHost } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap, preparedInstances, setInstanceOrder } from "$lib/stores/instance.svelte";
	import { lobbyServerProcessesList } from "$lib/stores/processes.svelte";
	import { modDeveloper } from "$lib/stores/settings.svelte";
	import { moveSidebarTab, sidebarLayout, sidebarZones } from "$lib/stores/sidebar.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui.svelte";
	import { createReorderable } from "$lib/utils/reorder.svelte";
	import SidebarInstanceItem from "./SidebarInstanceItem.svelte";
	import SidebarItem from "./SidebarItem.svelte";
	import { createTabDrag } from "./tabDrag.svelte";
	import type { SidebarTab, SidebarZone } from "$lib/stores/sidebar.svelte";
	import type { Instance } from "$lib/types/instance";

	const tabs: Record<
		SidebarTab,
		{ href: string; label: () => string; Icon: typeof House; shown?: () => boolean }
	> = {
		onboarding: {
			href: "/onboarding",
			label: () => m.sidebar_get_started(),
			Icon: Compass,
			shown: () => Object.keys(instanceMap.value).length === 0,
		},
		home: { href: "/", label: () => m.sidebar_home(), Icon: House },
		library: { href: "/library", label: () => m.sidebar_library(), Icon: Library },
		downloads: { href: "/downloads", label: () => m.sidebar_downloads(), Icon: Download },
		servers: { href: "/servers", label: () => m.sidebar_servers(), Icon: Server },
		converter: {
			href: "/converter",
			label: () => m.converter_title(),
			Icon: FlaskConical,
			shown: () => modDeveloper.value,
		},
		lobby: {
			href: "/lobby",
			label: () => m.sidebar_lobby(),
			Icon: Users,
			shown: () => !!lobbyHost.value,
		},
		logs: { href: "/logs", label: () => m.sidebar_logs(), Icon: ScrollText },
		settings: { href: "/settings", label: () => m.sidebar_settings(), Icon: Settings },
	};
	const isShown = (tab: SidebarTab) => tabs[tab].shown?.() ?? true;

	let asideEl: HTMLElement | undefined = $state();
	const tabDrag = createTabDrag({
		container: () => asideEl,
		onDrop: (id, { zone, index }) => moveSidebarTab(id, zone, index, isShown),
	});

	// While a tab is dragged it already sits where it would land.
	const zoneTabs = $derived.by(() => {
		const layout = sidebarLayout.value;
		const { drag, target } = tabDrag;
		return Object.fromEntries(
			sidebarZones.map((zone) => {
				const shown = layout[zone].filter(isShown);
				if (!drag) return [zone, shown];
				const rest = shown.filter((tab) => tab !== drag.id);
				if (target?.zone === zone) rest.splice(target.index, 0, drag.id);
				return [zone, rest];
			}),
		) as Record<SidebarZone, SidebarTab[]>;
	});

	let listEl: HTMLDivElement | undefined = $state();
	const reorder = createReorderable<Instance>({
		ids: () => preparedInstances.value.map((i) => i.id),
		container: () => listEl,
		onReorder: setInstanceOrder,
	});
</script>

{#snippet zone(name: SidebarZone, spacing = "")}
	<div
		data-zone={name}
		class={["flex flex-col items-center gap-2", zoneTabs[name].length > 0 && spacing]}
	>
		{#each zoneTabs[name] as id (id)}
			{@const tab = tabs[id]}
			<div
				data-tab={id}
				class="tab-slot"
				class:is-ghost={tabDrag.drag?.id === id}
				animate:flip={{ duration: 150 }}
			>
				<SidebarItem
					href={tab.href}
					label={tab.label()}
					onpointerdown={tabDrag.pointerdown(id)}
				>
					<tab.Icon size={20} />
				</SidebarItem>
			</div>
		{/each}
	</div>
{/snippet}

<aside
	bind:this={asideEl}
	data-sidebar
	class="bg-base-300 flex h-screen w-16 flex-none flex-col items-center py-4"
	class:dragging={!!reorder.drag}
>
	{@render zone("top")}

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="flex min-h-0 w-full flex-1 flex-col items-center">
		{@render zone("middleTop", "mb-2")}
		<div
			bind:this={listEl}
			class="instance-list flex min-h-0 flex-1 scrollbar-none flex-col gap-2 overflow-x-visible overflow-y-auto px-2"
		>
			{#each preparedInstances.value as instance, i (instance.id)}
				<div
					data-id={instance.id}
					class="instance-slot"
					style:transform={reorder.shiftFor(i)}
					class:is-ghost={reorder.drag?.id === instance.id}
				>
					<SidebarInstanceItem
						{instance}
						href={`/instance/${instance.id}`}
						active={page.route.id == "/instance/[id]" && page.params.id == instance.id}
						onpointerdown={reorder.pointerdown(instance.id, i, instance)}
					/>
				</div>
			{/each}

			<button
				class="btn btn-ghost btn-square outline-none"
				onclick={() => (instanceWizardOpen.value = true)}
			>
				<Plus size={20} />
			</button>
		</div>
		{@render zone("middleBottom", "mt-2")}
	</div>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div class="mt-auto flex flex-col gap-2">
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
		{@render zone("bottom")}
	</div>

	{#if reorder.drag}
		<div
			class="drag-clone pointer-events-none fixed z-[100]"
			style:top={`${reorder.pointerY - reorder.drag.offsetY}px`}
			style:left={`${reorder.pointerX - reorder.drag.offsetX}px`}
			aria-hidden="true"
			inert
		>
			<SidebarInstanceItem
				instance={reorder.drag.item}
				href={`/instance/${reorder.drag.item.id}`}
				active={true}
			/>
		</div>
	{/if}

	{#if tabDrag.drag}
		{@const tab = tabs[tabDrag.drag.id]}
		<div
			class="drag-clone pointer-events-none fixed z-[100]"
			style:top={`${tabDrag.pointerY - tabDrag.drag.offsetY}px`}
			style:left={`${tabDrag.pointerX - tabDrag.drag.offsetX}px`}
			aria-hidden="true"
			inert
		>
			<SidebarItem href={tab.href} label={tab.label()}><tab.Icon size={20} /></SidebarItem>
		</div>
	{/if}
</aside>

<style>
	.instance-slot,
	.tab-slot {
		touch-action: none;
		position: relative;
		z-index: 1;
	}
	/* Transitions only while dragging so the drop reorder is instant (no glitch). */
	.dragging .instance-slot {
		will-change: transform;
		transition: transform 200ms ease;
	}
	.instance-slot.is-ghost,
	.tab-slot.is-ghost {
		z-index: 0;
		opacity: 0.3;
		border-radius: 0.5rem;
	}
	.drag-clone {
		filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.45));
		transform: scale(1.08);
		transform-origin: center center;
		opacity: 0.95;
	}
</style>
