<script lang="ts">
	import { Plus } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { UpkMethods } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	type Vendor = UpkMethods["mcts.vendor"]["result"];
	let vendor = $state<Vendor | null>(null);
	let loading = $state(false);
	let busy = $state<number | null>(null);
	let filter = $state("");

	const visible = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		const missing = vendor?.missing ?? [];
		if (q === "") return missing;
		return missing.filter(
			(m) =>
				m.reference.toLowerCase().includes(q) ||
				m.name.toLowerCase().includes(q) ||
				String(m.bot).includes(q),
		);
	});

	async function load() {
		loading = true;
		try {
			vendor = await rpc("mcts.vendor", {});
		} catch (error) {
			toasts.error(error, "Could not read the Champion Vendor");
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (open) void load();
	});

	async function add(bot: number, label: string) {
		busy = bot;
		try {
			const r = await rpc("mcts.vendorAdd", { bot });
			if (r.existed) {
				toasts.push("info", `${label} is already in the vendor`, "Champion Vendor");
			} else {
				toasts.push(
					"success",
					`${label} (item ${r.item}) added; save the assembly to keep it`,
					"Champion Vendor",
				);
			}
			await load();
			if (r.vendor) {
				const [table, index] = r.vendor.at(-1)!;
				await assembly.openTable("assembly", table, r.vendor.slice(0, -1), index);
			}
		} catch (error) {
			toasts.error(error, "Could not add to the vendor");
		} finally {
			busy = null;
		}
	}
</script>

<Modal bind:open title="Champion Vendor" width="max-w-xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			The client builds its champion list (lobby and champion select, offline too) from the
			active entries of loot table 475, "Champion Vendor", one per Character Class item. A
			champion without an entry exists in the game but is never listed. The in-match champion
			select has 60 buttons: past 60 active entries, deactivate one (ACTIVE_FLAG) to free a
			slot.
		</p>
		{#if vendor}
			<div class="flex items-center gap-2 text-xs">
				<span class="badge badge-ghost badge-sm font-mono"
					>{vendor.active} active / {vendor.total} entries</span
				>
				{#if vendor.active >= 60}
					<span class="text-warning">all 60 in-match slots are taken</span>
				{/if}
				<div class="flex-1"></div>
				{#if vendor.index !== null}
					{@const index = vendor.index}
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => {
							open = false;
							void assembly.openTable(
								"assembly",
								"DATA_SET_LOOT_TABLE_ITEMS",
								[["DATA_SET_LOOT_TABLES", index]],
								null,
							);
						}}
						title="Open the vendor's entries in the grid">Open table</button
					>
				{/if}
			</div>
		{/if}
		<div class="flex items-center gap-2">
			<span class="text-xs opacity-70">Champions not in the vendor</span>
			<input
				class="input input-xs ml-auto w-48 font-mono"
				placeholder="Filter…"
				bind:value={filter}
			/>
		</div>
		{#if loading && !vendor}
			<div class="flex items-center gap-2 text-xs opacity-60">
				<span class="loading loading-xs loading-spinner"></span> Reading the assembly…
			</div>
		{:else if visible.length === 0}
			<p class="text-xs opacity-60">
				{vendor?.missing.length === 0
					? "Every champion bot with a Character Class item is listed."
					: "No match."}
			</p>
		{:else}
			<div class="max-h-80 overflow-auto">
				<table class="table-xs table">
					<thead
						><tr><th>bot</th><th>reference</th><th>name</th><th>item</th><th></th></tr
						></thead
					>
					<tbody>
						{#each visible as m (m.bot)}
							<tr>
								<td class="font-mono opacity-60">{m.bot}</td>
								<td class="font-mono">{m.reference}</td>
								<td>{m.name}</td>
								<td class="font-mono opacity-60">{m.item}</td>
								<td class="text-right">
									<button
										class="btn btn-primary btn-xs"
										disabled={busy !== null}
										onclick={() => add(m.bot, m.name || m.reference)}
									>
										{#if busy === m.bot}<span
												class="loading loading-xs loading-spinner"
											></span>{:else}<Plus size={11} />{/if}
										Add
									</button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</Modal>
