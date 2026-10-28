<script lang="ts">
	import { Code2, Map as MapIcon } from "@lucide/svelte";
	import { packageMenu } from "$lib/studio/contextmenus";
	import { bytes } from "$lib/studio/format";
	import { inField, reveal, step } from "$lib/studio/keys";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	let filter = $state("");
	let kind = $state<"all" | "content" | "maps" | "script">("all");

	const visible = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return studio.packages.filter((p) => {
			if (kind === "maps" && !p.map) return false;
			if (kind === "script" && !p.script) return false;
			if (kind === "content" && (p.script || p.map)) return false;
			return q === "" || p.name.toLowerCase().includes(q);
		});
	});

	function onKey(event: KeyboardEvent) {
		if (inField(event) || event.altKey || event.ctrlKey || event.metaKey) return;
		if (
			event.key !== "ArrowDown" &&
			event.key !== "ArrowUp" &&
			event.key !== "Home" &&
			event.key !== "End"
		) {
			return;
		}
		event.preventDefault();
		const at = visible.findIndex((p) => p.path === studio.pkg?.path);
		const i =
			event.key === "Home"
				? 0
				: event.key === "End"
					? visible.length - 1
					: step(at, event.key === "ArrowDown" ? 1 : -1, visible.length);
		const next = visible[i];
		if (!next || next.path === studio.pkg?.path) return;
		void studio.openPackage(next.path);
		reveal(`[data-package="${CSS.escape(next.path)}"]`);
	}
</script>

<div class="pane-title">
	Packages
	<span class="ml-auto font-mono normal-case opacity-60"
		>{visible.length}/{studio.packages.length}</span
	>
</div>
<div class="border-base-300 flex gap-1 border-b p-2">
	<input
		class="input input-sm flex-1 font-mono text-xs"
		placeholder="Filter…"
		bind:value={filter}
	/>
	<select class="select select-sm w-24 text-xs" bind:value={kind}>
		<option value="all">All</option>
		<option value="content">Content</option>
		<option value="maps">Maps</option>
		<option value="script">Script</option>
	</select>
</div>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="min-h-0 flex-1 overflow-y-auto" role="listbox" tabindex="-1" onkeydown={onKey}>
	{#each visible as p (p.path)}
		<button
			class="row-hover flex w-full items-center gap-2 px-3 py-1 text-left text-xs {studio.pkg
				?.path === p.path
				? 'row-selected'
				: ''}"
			data-package={p.path}
			onclick={(e) => {
				if (e.ctrlKey || e.metaKey) tabs.openInNewTab();
				void studio.openPackage(p.path);
			}}
			onauxclick={(e) => {
				if (e.button !== 1) return;
				tabs.openInNewTab();
				void studio.openPackage(p.path);
			}}
			ondblclick={() => studio.openPackage(p.path)}
			oncontextmenu={(e) => ui.openContextMenu(e, packageMenu(p.path))}
			title={p.path}
		>
			<span class="w-3 shrink-0 opacity-60">
				{#if p.script}<Code2 size={12} />{:else if p.map}<MapIcon size={12} />{/if}
			</span>
			<span class="truncate font-mono">{p.name}</span>
			{#if studio.dirty.includes(p.path)}
				<span
					class="text-warning"
					title={session.project ? "Edited (kept in your project)" : "Unsaved changes"}
					>●</span
				>
			{/if}
			{#if studio.marks.inPlace.includes(p.path)}
				<span
					class="badge badge-ghost badge-xs shrink-0"
					title="Saved in the game folder; the Studio keeps the original for mod deltas"
					>edited</span
				>
			{/if}
			{#if studio.marks.copies[p.path]}
				<span
					class="badge badge-ghost badge-xs shrink-0"
					title="Exported copies: {studio.marks.copies[p.path]?.join(', ')}">copy</span
				>
			{/if}
			{#if p.size > 0}
				<span class="ml-auto shrink-0 font-mono text-[10px] opacity-50"
					>{bytes(p.size)}</span
				>
			{:else}
				<span
					class="badge badge-ghost badge-xs ml-auto shrink-0"
					title="outside the workspace folder">ext</span
				>
			{/if}
		</button>
	{/each}
</div>
