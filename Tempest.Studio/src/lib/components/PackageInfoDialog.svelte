<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { studio } from "$lib/studio/state.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let tab = $state<"summary" | "imports" | "names">("summary");
	let filter = $state("");

	const s = $derived(studio.pkg?.summary);
	const imports = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return (studio.pkg?.imports ?? []).filter((i) => !q || i.path.toLowerCase().includes(q));
	});
	const names = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return (studio.pkg?.names ?? [])
			.map((n, i) => ({ n, i }))
			.filter((x) => !q || x.n.toLowerCase().includes(q));
	});
</script>

<Modal bind:open title="Package: {studio.pkg?.name ?? ''}" width="max-w-3xl">
	{#if studio.pkg && s}
		<div class="mb-2 flex items-center gap-2">
			<div role="tablist" class="tabs-box tabs tabs-sm">
				<button
					role="tab"
					class="tab {tab === 'summary' ? 'tab-active' : ''}"
					onclick={() => (tab = "summary")}>Summary</button
				>
				<button
					role="tab"
					class="tab {tab === 'imports' ? 'tab-active' : ''}"
					onclick={() => (tab = "imports")}>Imports ({s.imports})</button
				>
				<button
					role="tab"
					class="tab {tab === 'names' ? 'tab-active' : ''}"
					onclick={() => (tab = "names")}>Names ({s.names})</button
				>
			</div>
			{#if tab !== "summary"}
				<input
					class="input input-sm ml-auto w-64 font-mono text-xs"
					placeholder="Filter…"
					bind:value={filter}
				/>
			{/if}
		</div>
		{#if tab === "summary"}
			<table class="selectable table-xs table font-mono">
				<tbody>
					<tr><td class="opacity-60">file</td><td>{studio.pkg.path}</td></tr>
					<tr
						><td class="opacity-60">version</td><td
							>{s.fileVersion}/{s.licenseeVersion} · engine {s.engineVersion} · cooked content
							{s.cookedContentVersion}</td
						></tr
					>
					<tr><td class="opacity-60">guid</td><td>{s.guid}</td></tr>
					<tr><td class="opacity-60">folder</td><td>{s.folder || "—"}</td></tr>
					<tr
						><td class="opacity-60">package flags</td><td
							>{s.packageFlagsText} (0x{s.packageFlags.toString(16)})</td
						></tr
					>
					<tr
						><td class="opacity-60">storage</td><td
							>{s.compressed ? `LZO, ${s.chunks} chunks` : "uncompressed"}{s.obscured
								? ", obscured (XOR 0x2A)"
								: ""}</td
						></tr
					>
					<tr><td class="opacity-60">header</td><td>{s.headerSize} bytes</td></tr>
					<tr
						><td class="opacity-60">tables</td><td
							>{s.names} names · {s.imports} imports · {s.exports} exports · {s.textureAllocations}
							texture allocations</td
						></tr
					>
					{#if s.additionalPackages.length}
						<tr
							><td class="opacity-60">additional packages</td><td
								>{s.additionalPackages.join(", ")}</td
							></tr
						>
					{/if}
				</tbody>
			</table>
		{:else if tab === "imports"}
			<table class="selectable table-xs table font-mono">
				<thead><tr><th>#</th><th>class</th><th>path</th></tr></thead>
				<tbody>
					{#each imports.slice(0, 2000) as im (im.i)}
						<tr
							><td class="opacity-50">{im.i}</td><td>{im.classPackage}.{im.class}</td
							><td>{im.path}</td></tr
						>
					{/each}
				</tbody>
			</table>
		{:else}
			<table class="selectable table-xs table font-mono">
				<tbody>
					{#each names.slice(0, 3000) as x (x.i)}
						<tr><td class="opacity-50">{x.i}</td><td>{x.n}</td></tr>
					{/each}
				</tbody>
			</table>
		{/if}
	{/if}
</Modal>
