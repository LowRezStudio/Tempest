<script lang="ts">
	import { CopyPlus, Plus } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { McsPath, UpkMethods } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	type Data = UpkMethods["mcts.collections"]["result"];
	let data = $state<Data | null>(null);
	let loading = $state(false);
	let busy = $state<number | null>(null);
	let bot = $state("");
	let filter = $state("");

	const champions = $derived(
		Object.entries(data?.bots ?? {})
			.map(([id, name]) => ({ id: Number(id), name }))
			.sort((a, b) => a.name.localeCompare(b.name)),
	);
	const matches = (text: string) => {
		const q = filter.trim().toLowerCase();
		return q === "" || text.toLowerCase().includes(q);
	};
	const shown = $derived(
		(data?.collections ?? []).filter(
			(c) =>
				(!bot || String(c.bot) === bot) &&
				(matches(c.name) ||
					c.parts.some((p) => matches(p.name)) ||
					matches(String(c.item))),
		),
	);
	const loose = $derived(
		(data?.loose ?? []).filter(
			(s) => (!bot || String(s.bot) === bot) && (matches(s.name) || matches(String(s.item))),
		),
	);

	async function load() {
		loading = true;
		try {
			data = await rpc("mcts.collections", {});
		} catch (error) {
			toasts.error(error, "Could not read the collections");
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (open) void load();
	});

	async function go(path: McsPath | null) {
		if (!path) return;
		open = false;
		const [table, index] = path.at(-1)!;
		await assembly.openTable("assembly", table, path.slice(0, -1), index);
	}

	function copyOf(item: number) {
		ui.newContent = { kind: "skin", template: item };
		ui.open("newContent");
	}

	async function wrap(item: number, name: string) {
		busy = item;
		try {
			const r = await rpc("mcts.collectionAdd", { items: [item], name });
			toasts.push(
				"success",
				`${name} is now collection ${r.itemId} (loot table ${r.subTable}); save the assembly and the languages to keep it`,
				"Skin collections",
			);
			await load();
			await assembly.touched();
		} catch (error) {
			toasts.error(error, "Could not make a collection");
		} finally {
			busy = null;
		}
	}
</script>

<Modal bind:open title="Skin collections" width="max-w-3xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			The client lists a champion's skins from loot table 1076, "Inventory Vendor -
			Collections": each entry is a collection (a container item on the champion and a loot
			table with the body skin, head, weapon and entitlement). A skin no collection holds is
			never offered (default skins come from the champion's Default tables instead).
		</p>
		<div class="flex flex-wrap items-center gap-2">
			{#if data}
				<span class="badge badge-ghost badge-sm font-mono"
					>{data.active} active / {data.total} collections</span
				>
			{/if}
			<select class="select select-xs w-48" bind:value={bot}>
				<option value="">every champion</option>
				{#each champions as c (c.id)}
					<option value={String(c.id)}>{c.name} ({c.id})</option>
				{/each}
			</select>
			<input class="input input-xs w-48" placeholder="Filter…" bind:value={filter} />
			<div class="flex-1"></div>
			{#if data}
				{@const index = data.index}
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => go([["DATA_SET_LOOT_TABLES", index]])}
					title="Open loot table 1076 in the grid">Open table</button
				>
			{/if}
		</div>
		{#if loading && !data}
			<div class="flex items-center gap-2 text-xs opacity-60">
				<span class="loading loading-xs loading-spinner"></span> Reading the assembly…
			</div>
		{:else}
			<div class="max-h-[50vh] overflow-auto">
				<table class="table-xs table">
					<thead>
						<tr><th>collection</th><th>champion</th><th>parts</th><th></th></tr>
					</thead>
					<tbody>
						{#each shown as c, k (k)}
							<tr class:opacity-50={!c.active}>
								<td>
									<button
										class="link link-hover text-left"
										onclick={() => go(c.itemPath)}
										>{c.name || "(no name)"}</button
									>
									<span class="font-mono opacity-50">{c.item}</span>
									{#if !c.active}<span class="badge badge-ghost badge-xs"
											>inactive</span
										>{/if}
								</td>
								<td class="whitespace-nowrap"
									>{c.bot ? (data?.bots[c.bot] ?? c.bot) : ""}</td
								>
								<td>
									<div class="flex flex-wrap gap-1">
										{#each c.parts as p, k (k)}
											<span
												class="badge badge-sm {p.exists
													? 'badge-ghost'
													: 'badge-error'}"
												title="{p.name} ({p.item}){p.exists
													? ''
													: ': no such item'}">{p.kind}</span
											>
										{/each}
									</div>
								</td>
								<td class="text-right whitespace-nowrap">
									<button
										class="btn btn-ghost btn-xs"
										onclick={() => go(c.path)}
										title="The collection's entry in loot table 1076"
										>Entry</button
									>
									<button
										class="btn btn-ghost btn-xs"
										onclick={() => go(c.subPath)}
										disabled={!c.subPath}
										title="Its loot table of parts">Parts</button
									>
									<button
										class="btn btn-primary btn-xs"
										onclick={() => copyOf(c.item)}
										title="New content: a copy of this collection with every part"
										><CopyPlus size={11} /> New from this</button
									>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
				{#if shown.length === 0}<p class="p-2 text-xs opacity-60">
						No collection matches.
					</p>{/if}
				{#if loose.length > 0}
					<div class="mt-3 px-1 text-xs opacity-70">
						Skins no collection holds ({loose.length})
					</div>
					<table class="table-xs table">
						<tbody>
							{#each loose as s, k (k)}
								<tr>
									<td>
										<button
											class="link link-hover text-left"
											onclick={() => go(s.path)}
											>{s.name || "(no name)"}</button
										>
										<span class="font-mono opacity-50">{s.item}</span>
									</td>
									<td class="whitespace-nowrap"
										>{s.bot ? (data?.bots[s.bot] ?? s.bot) : ""}</td
									>
									<td class="text-right">
										<button
											class="btn btn-ghost btn-xs"
											disabled={busy !== null}
											onclick={() => wrap(s.item, s.name || `Skin ${s.item}`)}
											title="Make a collection holding this skin, so the client lists it"
										>
											{#if busy === s.item}<span
													class="loading loading-xs loading-spinner"
												></span>{:else}<Plus size={11} />{/if}
											Make a collection
										</button>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}
			</div>
		{/if}
	</div>
</Modal>
