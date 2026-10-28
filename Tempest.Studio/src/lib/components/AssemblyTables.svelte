<script lang="ts">
	import { Database, Languages } from "@lucide/svelte";
	import { assembly, versionRaw, versionText } from "$lib/studio/assembly.svelte";
	import { revealItem } from "$lib/studio/contextmenus";
	import { inField, reveal, step } from "$lib/studio/keys";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { McsDoc, McsHeaderField } from "../../../electron/src/ipc/contract";

	let filter = $state("");
	let editingHeader = $state<string | null>(null);

	const q = $derived(filter.trim().toLowerCase());

	function tablesOf(doc: McsDoc) {
		return doc.tables.filter((t) => q === "" || t.name.toLowerCase().includes(q));
	}

	function headerText(f: McsHeaderField): string {
		if (f.name === "GAME_VERSION_RAW" && f.value !== null && typeof f.value !== "object") {
			return versionText(f.value);
		}
		return f.value === null || typeof f.value === "object" ? "" : String(f.value);
	}

	async function commitHeader(doc: McsDoc, f: McsHeaderField, text: string) {
		editingHeader = null;
		let value = text;
		if (f.name === "GAME_VERSION_RAW") {
			const raw = versionRaw(text);
			if (!raw) return;
			value = raw;
		}
		if (
			value === headerText(f) ||
			(f.name === "GAME_VERSION_RAW" && value === String(f.value))
		) {
			return;
		}
		await assembly.setHeader(doc.id, f.name, value);
	}

	function onKey(event: KeyboardEvent) {
		if (inField(event) || event.altKey || event.ctrlKey || event.metaKey) return;
		if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
		event.preventDefault();
		const all = assembly.docs.flatMap((d) =>
			tablesOf(d).map((t) => ({ doc: d.id, table: t.name })),
		);
		const p = assembly.page;
		const at =
			p && p.path.length === 0
				? all.findIndex((x) => x.doc === p.doc && x.table === p.table)
				: -1;
		const next = all[step(at, event.key === "ArrowDown" ? 1 : -1, all.length)];
		if (!next) return;
		void assembly.openTable(next.doc, next.table, []);
		reveal(`[data-table="${next.doc}|${next.table}"]`);
	}

	const isCurrent = (doc: McsDoc, table: string) =>
		assembly.page?.doc === doc.id &&
		assembly.page.table === table &&
		assembly.page.path.length === 0;
</script>

<div class="pane-title">
	Tables
	<span class="ml-auto font-mono normal-case opacity-60">{assembly.docs.length} files</span>
</div>
<div class="border-base-300 flex gap-1 border-b p-2">
	<input
		class="input input-sm flex-1 font-mono text-xs"
		placeholder="Filter tables…"
		bind:value={filter}
	/>
</div>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="min-h-0 flex-1 overflow-y-auto" role="listbox" tabindex="-1" onkeydown={onKey}>
	{#each assembly.docs as doc (doc.id)}
		<div
			class="bg-base-200/60 sticky top-0 flex items-center gap-1.5 px-3 py-1 text-xs font-semibold"
			title={doc.path}
		>
			{#if doc.id === "assembly"}<Database size={12} />{:else}<Languages size={12} />{/if}
			<span class="truncate"
				>{doc.id === "assembly" ? "assembly.dat" : `Lang_${doc.id}.dat`}</span
			>
			{#if doc.dirty}<span class="text-warning">•</span>{/if}
			<span class="ml-auto font-mono font-normal opacity-50">{doc.function}</span>
		</div>
		{#if q === ""}
			{#each doc.header as f (f.name)}
				<div class="flex items-center gap-2 px-3 py-0.5 font-mono text-[11px]">
					<span class="w-32 shrink-0 truncate opacity-60" title={f.name}>{f.name}</span>
					{#if editingHeader === `${doc.id}|${f.name}`}
						<!-- svelte-ignore a11y_autofocus -->
						<input
							class="input input-xs flex-1"
							autofocus
							value={headerText(f)}
							onblur={(e) => commitHeader(doc, f, e.currentTarget.value)}
							onkeydown={(e) => {
								if (e.key === "Enter") e.currentTarget.blur();
								if (e.key === "Escape") editingHeader = null;
							}}
						/>
					{:else}
						<button
							class="hover:bg-base-300 flex-1 truncate rounded px-1 text-left"
							title={f.name === "GAME_VERSION_RAW"
								? `${f.value} (click to edit as major.minor.build.rev)`
								: `${headerText(f)} (click to edit)`}
							onclick={() => (editingHeader = `${doc.id}|${f.name}`)}
						>
							{headerText(f)}
						</button>
					{/if}
				</div>
			{/each}
		{/if}
		{#each tablesOf(doc) as t (t.name)}
			<button
				class="row-hover flex w-full items-center gap-2 px-3 py-1 text-left text-xs {isCurrent(
					doc,
					t.name,
				)
					? 'row-selected'
					: ''}"
				data-table="{doc.id}|{t.name}"
				onclick={(e) => {
					if (e.ctrlKey || e.metaKey) tabs.openInNewTab();
					void assembly.openTable(doc.id, t.name, []);
				}}
				oncontextmenu={(e) =>
					ui.openContextMenu(e, [
						{ heading: t.name },
						{
							label: "Open",
							run: () => assembly.openTable(doc.id, t.name, []),
						},
						{
							label: "Open in a new tab",
							run: () => {
								tabs.openInNewTab();
								return assembly.openTable(doc.id, t.name, []);
							},
						},
						"-",
						{
							label: "Copy the table name",
							run: () => navigator.clipboard.writeText(t.name),
						},
						revealItem(doc.path),
					])}
				title="{t.columns.length} columns"
			>
				<span class="truncate font-mono">{t.name.replace(/^DATA_SET_/, "")}</span>
				<span class="ml-auto shrink-0 font-mono text-[10px] opacity-50">{t.rows}</span>
			</button>
		{/each}
	{/each}
</div>
