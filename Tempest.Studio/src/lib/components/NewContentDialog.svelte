<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { McsCell } from "../../../electron/src/ipc/contract";

	type Kind = "device" | "card" | "item" | "champion";
	let {
		open = $bindable(false),
		kind = $bindable<Kind>("device"),
	}: { open?: boolean; kind?: Kind } = $props();

	let name = $state("");
	let description = $state("");
	let shortDescription = $state("");
	let template = $state("");
	let bot = $state("");
	let slot = $state("");
	let busy = $state(false);
	let champions = $state<{ id: number; name: string }[]>([]);
	let slots = $state<{ id: number; text: string }[]>([]);

	const KINDS: { id: Kind; label: string; table: string; key: string; hint: string }[] = [
		{
			id: "device",
			label: "Device (ability, weapon)",
			table: "DATA_SET_DEVICES",
			key: "DEVICE_ID",
			hint: "A copy of the template device and its item, with new ids, your name and description in every language, on the chosen champion and slot. Its modes, effect groups and classes are the template's: edit them in the grid, or point the class resources at the scaffold's classes.",
		},
		{
			id: "card",
			label: "Card / talent",
			table: "DATA_SET_DEVICES",
			key: "DEVICE_ID",
			hint: "Loadout cards and talents are devices too (item subtype Card, in a card or talent slot). Pick an existing card as template.",
		},
		{
			id: "item",
			label: "Item (skin, emote, spray…)",
			table: "DATA_SET_ITEMS",
			key: "ITEM_ID",
			hint: "A copy of the template item (and its skin row when it is a skin) with new ids and messages.",
		},
		{
			id: "champion",
			label: "Champion",
			table: "DATA_SET_BOTS",
			key: "BOT_ID",
			hint: "A copy of the template champion: its bot row, one new device per slot, the Character Class and Weapon items and the default skin. The private server must also list it (Export mod… writes paserver.json).",
		},
	];
	const current = $derived(KINDS.find((k) => k.id === kind) ?? KINDS[0]);

	/** The selected grid row as template when it is of the right table. */
	function fromSelection() {
		const p = assembly.page;
		if (!p || assembly.selected === null || p.table !== current.table || p.path.length > 0) {
			return;
		}
		const cells = p.rows.find(([i]) => i === assembly.selected)?.[1];
		const id = cells?.[current.key];
		if (typeof id === "number") template = String(id);
	}

	async function loadLists() {
		try {
			const bots = await rpc("mcts.rows", {
				doc: "assembly",
				table: "DATA_SET_BOTS",
				limit: 1000,
				sort: "REFERENCE_NAME",
			});
			const champs = bots.rows
				.filter(([, c]) => c.BOT_TYPE_VALUE_ID === 10849)
				.map(([, c]) => ({
					id: c.BOT_ID as number,
					name: String(c.REFERENCE_NAME ?? c.BOT_ID),
				}));
			champions = champs;
			await assembly.loadValidValues();
			const vv = assembly.validValues ?? {};
			slots = Object.entries(vv)
				.filter(([, v]) => /^ES\d+/.test(v.text) || /Card|Talent/i.test(v.text))
				.map(([id, v]) => ({ id: Number(id), text: v.text }))
				.sort((a, b) => a.text.localeCompare(b.text, undefined, { numeric: true }));
		} catch (error) {
			toasts.error(error, "Could not load the lists");
		}
	}

	$effect(() => {
		if (open) {
			fromSelection();
			if (champions.length === 0) void loadLists();
		}
	});

	async function create() {
		const t = Number(template);
		if (!Number.isFinite(t) || !name.trim()) return;
		busy = true;
		try {
			const r = await rpc("mcts.create", {
				kind,
				template: t,
				name: name.trim(),
				description,
				shortDescription,
				bot: bot ? Number(bot) : undefined,
				slot: slot ? Number(slot) : undefined,
			});
			const made = (r as Record<string, McsCell | unknown>)[
				kind === "champion" ? "bot" : kind === "item" ? "item" : "device"
			] as [string, number][] | undefined;
			toasts.push(
				"success",
				`${name} created (${kind === "champion" ? `bot ${r.botId}` : `id ${r.itemId ?? r.deviceId}`}); save the assembly and the languages to keep it`,
				"Created",
			);
			open = false;
			await (made?.[0]
				? assembly.openTable("assembly", made[0][0], [], made[0][1])
				: assembly.reload());
		} catch (error) {
			toasts.error(error, "Could not create");
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title="New content" width="max-w-xl">
	<div class="space-y-3 text-sm">
		<div class="join w-full">
			{#each KINDS as k (k.id)}
				<button
					class="btn join-item btn-sm flex-1 {kind === k.id ? 'btn-active' : ''}"
					onclick={() => {
						kind = k.id;
						fromSelection();
					}}>{k.label.split(" (")[0]}</button
				>
			{/each}
		</div>
		<p class="text-xs opacity-70">{current.hint}</p>
		<label class="flex flex-col gap-1">
			<span class="text-xs opacity-70"
				>Template ({current.key} of an existing row; select a row in the {current.table.replace(
					/^DATA_SET_/,
					"",
				)} table to fill it)</span
			>
			<input
				class="input input-sm font-mono"
				bind:value={template}
				placeholder="e.g. 18235"
			/>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs opacity-70"
				>Name (INT; the other languages start with the same text)</span
			>
			<input class="input input-sm" bind:value={name} placeholder="Flame Dash" />
		</label>
		{#if kind !== "champion"}
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Short description</span>
				<input class="input input-sm" bind:value={shortDescription} />
			</label>
		{/if}
		<label class="flex flex-col gap-1">
			<span class="text-xs opacity-70">Description</span>
			<textarea class="textarea textarea-sm" rows="3" bind:value={description}></textarea>
		</label>
		{#if kind !== "champion"}
			<div class="grid grid-cols-2 gap-2">
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Champion (REF_BOT_ID)</span>
					<select class="select select-sm" bind:value={bot}>
						<option value="">keep the template's</option>
						{#each champions as c (c.id)}
							<option value={String(c.id)}>{c.name} ({c.id})</option>
						{/each}
					</select>
				</label>
				{#if kind !== "item"}
					<label class="flex flex-col gap-1">
						<span class="text-xs opacity-70">Slot</span>
						<select class="select select-sm" bind:value={slot}>
							<option value="">keep the template's</option>
							{#each slots as s (s.id)}
								<option value={String(s.id)}>{s.text}</option>
							{/each}
						</select>
					</label>
				{/if}
			</div>
		{/if}
		<div class="flex justify-end gap-2">
			<button class="btn btn-sm" onclick={() => (open = false)}>Cancel</button>
			<button
				class="btn btn-sm btn-primary"
				onclick={create}
				disabled={busy || !template || !name.trim()}
			>
				{#if busy}<span class="loading loading-xs loading-spinner"></span>{/if}
				Create
			</button>
		</div>
	</div>
</Modal>
