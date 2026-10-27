<!-- One tab per place the user keeps open; the active tab follows the
	window's moves. Middle-click closes, Ctrl+W / Ctrl+Tab from the keyboard. -->
<script lang="ts">
	import { Database, FileBox, Plus, X } from "@lucide/svelte";
	import { keyLabel } from "$lib/studio/commands.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
</script>

{#if tabs.list.length > 0}
	<div
		class="border-base-300 bg-base-200 flex h-8 shrink-0 items-stretch overflow-x-auto border-b"
		role="tablist"
		aria-label="Open places"
	>
		{#each tabs.list as t (t.id)}
			{@const l = tabs.label(t.place)}
			{@const active = tabs.active === t.id}
			<div
				class="group border-base-300 flex max-w-56 min-w-28 shrink-0 items-center gap-1.5 border-r pr-1 pl-2.5 text-xs {active
					? 'bg-base-100 text-base-content border-t-primary border-t-2'
					: 'opacity-70 hover:opacity-100'}"
				role="tab"
				tabindex="0"
				aria-selected={active}
				title={l.detail}
				onclick={() => tabs.activate(t.id)}
				onkeydown={(e) => (e.key === "Enter" || e.key === " ") && tabs.activate(t.id)}
				onauxclick={(e) => e.button === 1 && tabs.close(t.id)}
			>
				{#if t.place.kind === "assembly"}<Database
						size={12}
						class="shrink-0 opacity-70"
					/>{:else}<FileBox size={12} class="shrink-0 opacity-70" />{/if}
				<span class="flex-1 truncate font-mono">{l.title}</span>
				<button
					class="btn btn-ghost btn-xs btn-square h-5 min-h-5 w-5 {active
						? ''
						: 'invisible group-hover:visible'}"
					onclick={(e) => {
						e.stopPropagation();
						void tabs.close(t.id);
					}}
					title="Close ({keyLabel('Mod+W')})"
					aria-label="Close tab"
				>
					<X size={11} />
				</button>
			</div>
		{/each}
		<button
			class="btn btn-ghost btn-xs btn-square m-1 self-center opacity-60"
			onclick={() => tabs.duplicate()}
			title="Duplicate this tab ({keyLabel(
				'Mod+T',
			)}); Ctrl+click a package, export or reference opens one"
			aria-label="New tab"
		>
			<Plus size={12} />
		</button>
	</div>
{/if}
