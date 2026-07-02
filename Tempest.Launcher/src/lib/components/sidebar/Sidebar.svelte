<script lang="ts">
	import {
		Box,
		Compass,
		Download,
		House,
		Library,
		Plus,
		ScrollText,
		Server,
		Settings,
		SquareTerminal,
		Terminal,
	} from "@lucide/svelte";
	import { page } from "$app/state";
	import { lobbyHost } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap, instanceOrder, setInstanceOrder } from "$lib/stores/instance.svelte";
	import { lobbyServerProcessesList } from "$lib/stores/processes.svelte";
	import { commandsPageOpen, instanceWizardOpen } from "$lib/stores/ui.svelte";
	import { getInstanceColor } from "$lib/utils/color";
	import { createReorderable } from "$lib/utils/reorder.svelte";
	import LanguageSelector from "./LanguageSelector.svelte";
	import SidebarItem from "./SidebarItem.svelte";
	import type { Instance } from "$lib/types/instance";

	let preparedInstances = $derived.by(() => {
		const order = instanceOrder.value;
		const all = Object.values(instanceMap.value).filter(
			(i) => !!i && i.state?.type === "prepared",
		);
		const byId = new Map(all.map((i) => [i.id, i]));
		const sorted: Instance[] = [];
		for (const id of order) {
			const inst = byId.get(id);
			if (inst) {
				sorted.push(inst);
				byId.delete(id);
			}
		}
		for (const inst of byId.values()) sorted.push(inst);
		return sorted;
	});

	let listEl: HTMLDivElement | undefined = $state();
	const reorder = createReorderable<Instance>({
		ids: () => preparedInstances.map((i) => i.id),
		container: () => listEl,
		onReorder: setInstanceOrder,
	});
</script>

<aside
	class="flex h-screen w-16 flex-none flex-col items-center bg-base-300 py-4"
	class:dragging={!!reorder.drag}
>
	<nav class="flex flex-col gap-2">
		<SidebarItem href="/" icon={House} label={m.sidebar_home()} />
		<SidebarItem href="/library" icon={Library} label={m.sidebar_library()} />
		<SidebarItem href="/downloads" icon={Download} label={m.sidebar_downloads()} />
		<SidebarItem href="/servers" icon={Server} label={m.sidebar_servers()} />
		{#if lobbyHost.value}
			<SidebarItem href="/lobby" icon={Compass} label={m.sidebar_lobby()} />
		{/if}
	</nav>

	<div class="divider mx-4 my-4 opacity-50"></div>

	<div
		bind:this={listEl}
		class="instance-list flex flex-1 flex-col gap-2 overflow-y-auto overflow-x-visible px-2 scrollbar-none"
	>
		{#each preparedInstances as instance, i (instance.id)}
			<div
				data-id={instance.id}
				class="instance-slot"
				style:transform={reorder.shiftFor(i)}
				class:is-ghost={reorder.drag?.id === instance.id}
			>
				<SidebarItem
					icon={Box}
					label={instance.label}
					active={page.route.id == "/instance/[id]" && page.params.id == instance.id}
					href={`/instance/${instance.id}`}
					color={getInstanceColor(instance)}
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
	<div class="flex flex-col gap-2 overflow-y-auto overflow-x-visible px-2 scrollbar-none">
		{#if commandsPageOpen.value}
			<SidebarItem
				icon={Terminal}
				label={m.commands_page_title()}
				active={page.url.pathname === "/news"}
				href="/news"
			/>
		{/if}
		{#each lobbyServerProcessesList.value as lobbyServer}
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
		<SidebarItem href="/logs" icon={ScrollText} label={m.sidebar_logs()} />
		<SidebarItem href="/settings" icon={Settings} label={m.sidebar_settings()} />
	</div>

	{#if reorder.drag}
		<div
			class="drag-clone pointer-events-none fixed z-[100]"
			style:top={`${reorder.pointerY - reorder.drag.offsetY}px`}
			style:left={`${reorder.pointerX - reorder.drag.offsetX}px`}
			aria-hidden="true"
			inert
		>
			<SidebarItem
				icon={Box}
				label={reorder.drag.item.label}
				href={`/instance/${reorder.drag.item.id}`}
				color={getInstanceColor(reorder.drag.item)}
			/>
		</div>
	{/if}
</aside>

<style>
	.instance-slot {
		touch-action: none;
		position: relative;
		z-index: 1;
	}
	/* Transitions only while dragging so the drop reorder is instant (no glitch). */
	.dragging .instance-slot {
		will-change: transform;
		transition: transform 200ms ease;
	}
	.instance-slot.is-ghost {
		z-index: 0;
		opacity: 0.3;
		outline: 2px dashed currentColor;
		outline-offset: -2px;
		border-radius: 0.5rem;
	}
	.drag-clone {
		filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.45));
		transform: scale(1.08);
		transform-origin: center center;
		opacity: 0.95;
	}
</style>
