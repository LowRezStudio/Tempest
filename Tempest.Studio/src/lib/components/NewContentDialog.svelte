<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { McsCell, McsCollection, UpkMethods } from "../../../electron/src/ipc/contract";

	type Kind = "device" | "card" | "item" | "champion" | "skin";
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
	/** The id range setting as typed (empty = past the largest id in use). */
	let rangeStart = $state(studio.idRange ? String(studio.idRange.start) : "");
	let rangeEnd = $state(studio.idRange ? String(studio.idRange.end) : "");
	let nextIds = $state<{ item: number; bot: number; msg: number } | null>(null);
	let rangeError = $state<string | null>(null);

	/** Save the typed range (both fields, start < end) and tell the backend; refreshes the next ids. */
	async function applyRange() {
		const s = Number(rangeStart);
		const e = Number(rangeEnd);
		if (rangeStart === "" && rangeEnd === "") studio.idRange = null;
		else if (Number.isInteger(s) && Number.isInteger(e) && s >= 0 && e > s) {
			studio.idRange = { start: s, end: e };
		} else {
			rangeError = "start and end must be whole numbers with start < end";
			return;
		}
		rangeError = null;
		await assembly.applyIdRange();
		await loadNextIds();
	}

	async function loadNextIds() {
		try {
			const n = await rpc("mcts.nextIds", {});
			nextIds = { item: n.item, bot: n.bot, msg: n.msg };
			rangeError = null;
		} catch (error) {
			nextIds = null;
			rangeError = error instanceof Error ? error.message : String(error);
		}
	}

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
			id: "skin",
			label: "Skin (collection)",
			table: "DATA_SET_ITEMS",
			key: "ITEM_ID",
			hint: "Skins are sold and listed per champion as collections (loot table 1076, Inventory Vendor - Collections): a container item, a loot table with the body skin, head, weapon and entitlement, and the vendor entry. This copies a collection with all of its parts (their skin rows too), names the parts after yours and adds the vendor entry. The copies show the template's meshes until you point their skin rows at yours.",
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
			const preset = ui.newContent;
			if (preset) {
				kind = preset.kind;
				template = String(preset.template);
				ui.newContent = null;
			} else fromSelection();
			if (champions.length === 0) void loadLists();
			void loadNextIds();
		}
	});

	// ----- skins ----------------------------------------------------------------
	/** Champion whose collections the template list shows. */
	let skinBot = $state("");
	let collections = $state<McsCollection[]>([]);
	let collectionsLoading = $state(false);
	let source = $state<UpkMethods["mcts.collection"]["result"] | null>(null);
	/** Part names typed by the user, by the template part's item id. */
	let partNames = $state<Record<number, string>>({});

	/** Same rule as `rename` in mcts/skins.rs. */
	function themeOf(text: string, champion: string): string {
		if (!champion) return text;
		return text.split(champion).join(" ").split(/\s+/).filter(Boolean).join(" ");
	}
	function swapText(text: string, from: string, to: string): string | null {
		if (!from || !text.includes(from)) return null;
		return text.split(from).join(to).split(/\s+/).filter(Boolean).join(" ");
	}
	function renamePart(text: string): string {
		if (!source) return text;
		const oldChamp = source.champion;
		const newChamp = bot
			? (champions.find((c) => String(c.id) === bot)?.name ?? oldChamp)
			: oldChamp;
		const oldTheme = source.theme;
		const newTheme = themeOf(name.trim(), newChamp);
		let out = text;
		let changed = oldTheme === newTheme;
		const a = oldTheme !== newTheme ? swapText(out, oldTheme, newTheme) : null;
		if (a !== null) {
			out = a;
			changed = true;
		}
		const b = oldChamp && oldChamp !== newChamp ? swapText(out, oldChamp, newChamp) : null;
		if (b !== null) {
			out = b;
			changed = true;
		}
		return changed ? out : `${newTheme} ${text}`.trim();
	}
	const partRows = $derived(
		(source?.parts ?? []).map((p) => ({
			...p,
			suggested: renamePart(p.name),
			value: partNames[p.item] ?? renamePart(p.name),
		})),
	);

	async function loadCollections() {
		collectionsLoading = true;
		try {
			const r = await rpc("mcts.collections", skinBot ? { bot: Number(skinBot) } : {});
			collections = r.collections;
		} catch (error) {
			toasts.error(error, "Could not read the collections");
		} finally {
			collectionsLoading = false;
		}
	}

	async function loadSource(item: number) {
		try {
			source = await rpc("mcts.collection", { item });
			partNames = {};
		} catch {
			source = null;
		}
	}

	$effect(() => {
		if (open && kind === "skin" && skinBot !== undefined) void loadCollections();
	});
	$effect(() => {
		const t = Number(template);
		if (open && kind === "skin" && template && Number.isFinite(t)) void loadSource(t);
		else source = null;
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
				names:
					kind === "skin"
						? Object.fromEntries(partRows.map((p) => [String(p.item), p.value]))
						: undefined,
			});
			const made = (r as Record<string, McsCell | unknown>)[
				kind === "champion" ? "bot" : kind === "item" || kind === "skin" ? "item" : "device"
			] as [string, number][] | undefined;
			toasts.push(
				"success",
				kind === "skin"
					? `${name} created: ${r.parts?.length ?? 0} items, loot table ${r.subTable} and a Collections vendor entry; save the assembly and the languages to keep it`
					: `${name} created (${kind === "champion" ? `bot ${r.botId}` : `id ${r.itemId ?? r.deviceId}`}); save the assembly and the languages to keep it`,
				"Created",
			);
			open = false;
			await assembly.touched();
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

<Modal bind:open title="New content" width="max-w-2xl">
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
		{#if kind === "skin"}
			<div class="grid grid-cols-[auto_1fr] items-center gap-2">
				<span class="text-xs opacity-70">Collections of</span>
				<select class="select select-sm" bind:value={skinBot}>
					<option value="">every champion</option>
					{#each champions as c (c.id)}
						<option value={String(c.id)}>{c.name} ({c.id})</option>
					{/each}
				</select>
				<span class="text-xs opacity-70">Template</span>
				<select
					class="select select-sm"
					bind:value={template}
					disabled={collectionsLoading}
				>
					<option value="">pick a collection…</option>
					{#each collections as c, k (k)}
						<option value={String(c.item)}
							>{c.name || `item ${c.item}`} ({c.item}){c.active
								? ""
								: " — inactive"}</option
						>
					{/each}
				</select>
			</div>
		{/if}
		<label class="flex flex-col gap-1" class:hidden={kind === "skin"}>
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
			<input
				class="input input-sm"
				bind:value={name}
				placeholder={kind === "skin" ? "Frost Evie" : "Flame Dash"}
			/>
		</label>
		{#if kind === "skin" && partRows.length > 0}
			<div class="flex flex-col gap-1">
				<span class="text-xs opacity-70"
					>Parts (copied from {source?.name}; names follow yours, edit any)</span
				>
				{#each partRows as p, k (k)}
					<div class="flex items-center gap-2">
						<span class="badge badge-ghost badge-sm w-20 shrink-0">{p.kind}</span>
						<input
							class="input input-xs flex-1"
							value={p.value}
							title="Template: {p.name} ({p.item})"
							oninput={(e) => (partNames[p.item] = e.currentTarget.value)}
						/>
					</div>
				{/each}
			</div>
		{/if}
		{#if kind !== "champion" && kind !== "skin"}
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Short description</span>
				<input class="input input-sm" bind:value={shortDescription} />
			</label>
		{/if}
		{#if kind !== "skin"}
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Description</span>
				<textarea class="textarea textarea-sm" rows="3" bind:value={description}></textarea>
			</label>
		{/if}
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
				{#if kind !== "item" && kind !== "skin"}
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
		<div class="bg-base-200 rounded-box space-y-1 p-2">
			<div class="flex flex-wrap items-center gap-2 text-xs">
				<span
					class="opacity-70"
					title="Ids of new rows and messages are taken from this range. Give every mod its own range (say 100000 wide) so mods made with the Studio never allocate the same ids; empty means one past the largest id in use."
				>
					Id range for new content
				</span>
				<input
					class="input input-xs w-28 font-mono"
					bind:value={rangeStart}
					placeholder="start"
					onchange={applyRange}
				/>
				<span class="opacity-50">…</span>
				<input
					class="input input-xs w-28 font-mono"
					bind:value={rangeEnd}
					placeholder="end"
					onchange={applyRange}
				/>
				{#if nextIds}
					<span class="ml-auto font-mono opacity-60"
						>next: item {nextIds.item}, bot {nextIds.bot}, msg {nextIds.msg}</span
					>
				{/if}
			</div>
			{#if rangeError}<div class="text-error text-xs">{rangeError}</div>{/if}
		</div>
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
