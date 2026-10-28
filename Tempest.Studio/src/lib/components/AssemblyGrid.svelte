<script lang="ts">
	import {
		ArrowDown,
		ArrowUp,
		ChevronLeft,
		ChevronRight,
		ClipboardCopy,
		Columns3,
		Copy,
		Pencil,
		CopyPlus,
		ExternalLink,
		Plus,
		Sparkles,
		Trash2,
		Users,
		X,
	} from "@lucide/svelte";
	import { assembly, isMessageColumn, refOf } from "$lib/studio/assembly.svelte";
	import { keyLabel } from "$lib/studio/commands.svelte";
	import { reveal } from "$lib/studio/keys";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { McsCell, McsColumn } from "../../../electron/src/ipc/contract";

	let filterText = $state("");
	let filterTimer: ReturnType<typeof setTimeout> | null = null;
	/** The cell being edited, `${index}|${column}`, and the text it starts with. */
	let editing = $state<string | null>(null);
	let editStart = $state<string | null>(null);
	/** The active column of the cell cursor (the active row is `assembly.selected`). */
	let col = $state<string | null>(null);
	/** Shift+click anchor for range marks. */
	let anchor: number | null = null;

	const page = $derived(assembly.page);
	const from = $derived(page ? Math.min(page.offset + 1, page.total) : 0);
	const to = $derived(page ? Math.min(page.offset + page.rows.length, page.total) : 0);

	// ----- columns: pinned id + name first, hidden ones per table (remembered)

	const HIDDEN_KEY = "studio.asm.hidden";
	let hiddenAll = $state<Record<string, string[]>>(
		JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "{}") as Record<string, string[]>,
	);
	const hidden = $derived(new Set(page ? (hiddenAll[page.table] ?? []) : []));

	function saveHidden(next: Record<string, string[]>) {
		hiddenAll = next;
		localStorage.setItem(HIDDEN_KEY, JSON.stringify(next));
	}

	function toggleColumn(name: string) {
		if (!page) return;
		const set = new Set(hiddenAll[page.table] ?? []);
		if (set.has(name)) set.delete(name);
		else set.add(name);
		saveHidden({ ...hiddenAll, [page.table]: [...set] });
	}

	/** The id column of a table from its name (DATA_SET_BOTS → BOT_ID). */
	function keyColumn(table: string, cols: McsColumn[]): string | null {
		const stem = table.replace(/^DATA_SET_/, "");
		const names = new Set(cols.map((c) => c.name));
		for (const c of [
			`${stem}_ID`,
			stem.endsWith("IES") ? `${stem.slice(0, -3)}Y_ID` : "",
			stem.endsWith("ES") ? `${stem.slice(0, -2)}_ID` : "",
			stem.endsWith("S") ? `${stem.slice(0, -1)}_ID` : "",
		]) {
			if (c && names.has(c)) return c;
		}
		return null;
	}

	const keyCol = $derived(page ? keyColumn(page.table, page.columns) : null);
	const nameCol = $derived(
		page
			? (["REFERENCE_NAME", "NAME", "MESSAGE"].find((n) =>
					page.columns.some((c) => c.name === n),
				) ?? null)
			: null,
	);
	const pinned = $derived([keyCol, nameCol].filter((c): c is string => c !== null));
	const columns = $derived.by<McsColumn[]>(() => {
		if (!page) return [];
		const first = pinned
			.map((n) => page.columns.find((c) => c.name === n))
			.filter((c): c is McsColumn => !!c);
		const rest = page.columns.filter((c) => !pinned.includes(c.name) && !hidden.has(c.name));
		return [...first, ...rest];
	});
	/** Widths (rem) of the pinned columns; the # column is 3rem. */
	const PIN_WIDTH = [7, 13];
	function stickyLeft(i: number): number | null {
		if (i >= pinned.length) return null;
		let left = 3;
		for (let k = 0; k < i; k++) left += PIN_WIDTH[k] ?? 8;
		return left;
	}

	$effect(() => {
		// The filter box follows the store (cleared when another table opens).
		filterText = assembly.filter;
	});

	$effect(() => {
		// Keep the cursor on a column of this table.
		if (page && (col === null || !columns.some((c) => c.name === col))) {
			col = columns[0]?.name ?? null;
		}
	});

	function onFilter(value: string) {
		filterText = value;
		if (filterTimer) clearTimeout(filterTimer);
		filterTimer = setTimeout(() => void assembly.setFilter(value), 250);
	}

	function text(cell: McsCell | undefined): string {
		if (cell === undefined || cell === null) return "";
		if (typeof cell === "object") return `${cell.rows} rows`;
		return String(cell);
	}

	function hint(column: McsColumn, cell: McsCell | undefined): string | null {
		if (cell === undefined || cell === null || typeof cell === "object") return null;
		if (isMessageColumn(column.name)) return assembly.message(cell);
		if (column.name.endsWith("_VALUE_ID") && assembly.validValues) {
			return assembly.validValues[String(cell)]?.text ?? null;
		}
		return null;
	}

	function editable(column: McsColumn): boolean {
		return column.kind !== "rowset" && column.kind !== "blob";
	}

	const isFlag = (name: string) => name.endsWith("_FLAG");

	function cellOf(index: number, name: string): McsCell | undefined {
		return page?.rows.find(([i]) => i === index)?.[1][name];
	}

	/** Valid values for a `*_VALUE_ID` editor: id → text. */
	const validList = $derived(
		assembly.validValues
			? Object.entries(assembly.validValues).map(([id, v]) => ({ id, text: v.text }))
			: [],
	);

	// ----- editing

	function startEdit(index: number, name: string, initial: string | null = null) {
		const c = page?.columns.find((x) => x.name === name);
		if (!c || !editable(c)) return;
		if (isFlag(name) && initial === null) {
			void toggleFlag(index, name);
			return;
		}
		editStart = initial;
		editing = `${index}|${name}`;
	}

	/** The rows an edit of row `index` applies to: the marked rows when it is one of them. */
	function editTargets(index: number): number[] {
		return assembly.marked.length > 1 && assembly.marked.includes(index)
			? assembly.marked
			: [index];
	}

	async function setOn(index: number, name: string, value: string) {
		const rows = editTargets(index);
		await (rows.length > 1
			? assembly.setMany(rows, name, value)
			: assembly.setCell(index, name, value));
	}

	async function commit(
		index: number,
		name: string,
		value: string,
		move: "down" | "right" | "left" | null,
	) {
		editing = null;
		editStart = null;
		if (value !== text(cellOf(index, name))) await setOn(index, name, value);
		focusGrid();
		if (move === "down") await selectAt(rowPos(index) + 1);
		else if (move === "right") moveCol(1);
		else if (move === "left") moveCol(-1);
	}

	async function toggleFlag(index: number, name: string) {
		await setOn(index, name, cellOf(index, name) === "y" ? "n" : "y");
	}

	async function openNested(index: number, column: McsColumn) {
		if (!page) return;
		await assembly.openTable(page.doc, column.name, [...page.path, [page.table, index]]);
	}

	function crumbs(): { label: string; path: [string, number][]; table: string }[] {
		if (!page) return [];
		const out: { label: string; path: [string, number][]; table: string }[] = [];
		for (let i = 0; i < page.path.length; i++) {
			const [table, index] = page.path[i];
			out.push({
				label: `${table.replace(/^DATA_SET_/, "")}[${index}]`,
				path: page.path.slice(0, i),
				table,
			});
		}
		return out;
	}

	$effect(() => {
		if (page?.doc === "assembly") void assembly.loadValidValues();
	});

	// ----- cursor and selection

	let gridEl = $state<HTMLDivElement | null>(null);

	function focusGrid() {
		gridEl?.focus({ preventScroll: true });
	}

	function rowPos(index: number): number {
		return page ? page.rows.findIndex(([i]) => i === index) : -1;
	}

	async function selectAt(pos: number) {
		if (!page) return;
		let target = pos;
		if (target < 0) {
			if (page.offset === 0) return;
			await assembly.prevPage();
			target = (assembly.page?.rows.length ?? 1) - 1;
		} else if (target >= page.rows.length) {
			if (page.offset + assembly.limit >= page.total) return;
			await assembly.nextPage();
			target = 0;
		}
		const row = assembly.page?.rows[target];
		if (!row) return;
		await assembly.select(row[0]);
		revealCell();
	}

	function moveCol(step: number) {
		const i = columns.findIndex((c) => c.name === col);
		const next = columns[Math.max(0, Math.min(columns.length - 1, i + step))];
		if (next) col = next.name;
		revealCell();
	}

	function revealCell() {
		requestAnimationFrame(() => {
			const sel = assembly.selected;
			if (sel === null) return;
			const el = gridEl?.querySelector(`[data-row="${sel}"] [data-col="${col}"]`);
			if (el) el.scrollIntoView({ block: "nearest", inline: "nearest" });
			else reveal(`[data-row="${sel}"]`, false);
		});
	}

	function onRowClick(event: MouseEvent, index: number, name: string | null) {
		focusGrid();
		if (name) col = name;
		if (event.shiftKey && anchor !== null && page) {
			const a = rowPos(anchor);
			const b = rowPos(index);
			const [lo, hi] = a < b ? [a, b] : [b, a];
			assembly.marked = page.rows.slice(lo, hi + 1).map(([i]) => i);
		} else if (event.ctrlKey || event.metaKey) {
			const base =
				assembly.marked.length === 0 && assembly.selected !== null
					? [assembly.selected]
					: assembly.marked;
			assembly.marked = base.includes(index)
				? base.filter((i) => i !== index)
				: [...base, index];
			anchor = index;
		} else {
			assembly.marked = [];
			anchor = index;
		}
		void assembly.select(index);
	}

	/** Right-click on a row: select it (unless it is one of the marked rows), then its actions. */
	function onRowMenu(event: MouseEvent, index: number) {
		const name =
			(event.target as Element | null)?.closest("[data-col]")?.getAttribute("data-col") ??
			null;
		if (!assembly.marked.includes(index) && assembly.selected !== index) {
			assembly.marked = [];
			anchor = index;
			void assembly.select(index);
		}
		if (name) col = name;
		const rows = assembly.marked.includes(index) ? assembly.marked : [index];
		const many = rows.length > 1;
		const column = name ? columns.find((c) => c.name === name) : undefined;
		const value = name ? text(cellOf(index, name)) : "";
		ui.openContextMenu(event, [
			{ heading: many ? `${rows.length} marked rows` : `Row #${index}` },
			column && editable(column)
				? {
						label: `Edit ${column.name}`,
						icon: Pencil,
						keys: "F2",
						run: () => startEdit(index, column.name),
					}
				: null,
			name
				? {
						label: "Copy the value",
						detail: value || "(empty)",
						icon: ClipboardCopy,
						run: () => navigator.clipboard.writeText(value),
					}
				: null,
			"-",
			{
				label: many ? "Copy the rows" : "Copy the row",
				detail: "To paste into this table as new rows",
				icon: Copy,
				keys: "Mod+C",
				run: copy,
			},
			{
				label: "Copy as TSV",
				detail: "For a spreadsheet",
				run: () =>
					assembly.copyTsv(
						assembly.targets(),
						columns.map((c) => c.name),
					),
			},
			{ label: "Paste", keys: "Mod+V", run: paste },
			"-",
			{
				label: "Duplicate",
				detail: "A copy at the end, with a fresh id",
				icon: CopyPlus,
				keys: "Mod+D",
				disabled: many,
				run: () => assembly.addRow(index),
			},
			{
				label: "New row",
				icon: Plus,
				keys: "Mod+Shift+Enter",
				run: () => assembly.addRow(null),
			},
			{
				label: "Open in a new tab",
				icon: ExternalLink,
				run: () => {
					tabs.openInNewTab();
					return assembly.select(index);
				},
			},
			"-",
			{
				label: many ? `Delete ${rows.length} rows…` : "Delete the row…",
				icon: Trash2,
				danger: true,
				run: deleteTargets,
			},
		]);
	}

	async function copy() {
		const rows = assembly.targets();
		if (rows.length === 0) return;
		const n = await assembly.copyRows(rows);
		toasts.push(
			"success",
			`${n} row(s) copied; paste them into this table with ${keyLabel("Mod+V")}`,
			"Copy",
		);
	}

	async function paste() {
		const clip = await navigator.clipboard.readText();
		if (await assembly.pasteRows(clip)) return;
		// Plain text: the value of the active cell (of every marked row).
		const sel = assembly.selected;
		if (sel === null || !col) return;
		const value = clip.replace(/\r?\n$/, "");
		if (value.includes("\n")) {
			toasts.push(
				"info",
				"The clipboard holds several lines; paste one value at a time.",
				"Paste",
			);
			return;
		}
		await setOn(sel, col, value);
	}

	async function deleteTargets() {
		const rows = assembly.targets();
		if (rows.length === 0 || !page) return;
		const ok = await ui.confirm({
			title: rows.length === 1 ? "Delete this row?" : `Delete ${rows.length} rows?`,
			message: `From ${page.table.replace(/^DATA_SET_/, "")}. Undo (Ctrl+Z) brings ${rows.length === 1 ? "it" : "them"} back until you discard.`,
			items: rows.slice(0, 50).map((i) => {
				const cells = page.rows.find(([x]) => x === i)?.[1];
				const label = (keyCol && cells?.[keyCol]) ?? "";
				const name = (nameCol && cells?.[nameCol]) ?? "";
				return `#${i} ${label} ${name}`.trim();
			}),
			confirm: "Delete",
			danger: true,
		});
		if (ok === true) await assembly.deleteRows(rows);
	}

	async function onModKey(event: KeyboardEvent, rows: [number, Record<string, McsCell>][]) {
		const k = event.key.toLowerCase();
		if (k === "c") {
			event.preventDefault();
			await copy();
		} else if (k === "v") {
			event.preventDefault();
			await paste();
		} else if (k === "a") {
			event.preventDefault();
			assembly.marked = rows.map(([i]) => i);
		}
	}

	async function onArrowRow(
		event: KeyboardEvent,
		next: number,
		rows: [number, Record<string, McsCell>][],
	) {
		event.preventDefault();
		if (event.shiftKey && assembly.selected !== null) {
			const row = rows[next];
			if (row) {
				const marks = new Set(
					assembly.marked.length > 0 ? assembly.marked : [assembly.selected],
				);
				marks.add(row[0]);
				assembly.marked = [...marks];
			}
		} else {
			assembly.marked = [];
		}
		await selectAt(next);
	}

	async function activate(event: KeyboardEvent) {
		if (!page || assembly.selected === null || !col) return;
		event.preventDefault();
		const c = page.columns.find((x) => x.name === col);
		const cell = cellOf(assembly.selected, col);
		if (c && cell !== null && typeof cell === "object") await openNested(assembly.selected, c);
		else startEdit(assembly.selected, col);
	}

	async function onKey(event: KeyboardEvent) {
		if (!page || editing || event.target !== gridEl) return;
		if (event.ctrlKey || event.metaKey) {
			await onModKey(event, page.rows);
			return;
		}
		if (event.altKey) return;
		const at = assembly.selected === null ? -1 : rowPos(assembly.selected);
		const key = event.key;
		switch (key) {
			case "ArrowDown": {
				await onArrowRow(event, at + 1, page.rows);
				break;
			}
			case "ArrowUp": {
				await onArrowRow(event, at < 0 ? 0 : at - 1, page.rows);
				break;
			}
			case "ArrowLeft":
			case "ArrowRight": {
				event.preventDefault();
				moveCol(key === "ArrowRight" ? 1 : -1);
				break;
			}
			case "Tab": {
				event.preventDefault();
				moveCol(event.shiftKey ? -1 : 1);
				break;
			}
			case "PageDown":
			case "PageUp": {
				event.preventDefault();
				await (key === "PageDown" ? assembly.nextPage() : assembly.prevPage());
				await selectAt(0);
				break;
			}
			case "Home":
			case "End": {
				event.preventDefault();
				await selectAt(key === "Home" ? 0 : page.rows.length - 1);
				break;
			}
			case "Enter":
			case "F2": {
				await activate(event);
				break;
			}
			case "Escape": {
				assembly.marked = [];
				break;
			}
			case "Delete": {
				event.preventDefault();
				await deleteTargets();
				break;
			}
			default: {
				if (assembly.selected === null || !col) return;
				if (key === " " && isFlag(col)) {
					event.preventDefault();
					await toggleFlag(assembly.selected, col);
				} else if (key.length === 1 && !isFlag(col)) {
					// Typing starts editing the active cell with that character.
					event.preventDefault();
					startEdit(assembly.selected, col, key);
				}
			}
		}
	}

	function onEditKey(e: KeyboardEvent, index: number, name: string) {
		e.stopPropagation();
		const input = e.currentTarget as HTMLInputElement;
		if (e.key === "Enter") {
			e.preventDefault();
			void commit(index, name, input.value, "down");
		} else if (e.key === "Tab") {
			e.preventDefault();
			void commit(index, name, input.value, e.shiftKey ? "left" : "right");
		} else if (e.key === "Escape") {
			e.preventDefault();
			editing = null;
			editStart = null;
			focusGrid();
		}
	}

	const markedSet = $derived(new Set(assembly.marked));

	/** Opaque background of a pinned cell, matching its row's highlight. */
	function pinBg(isSel: boolean, isMarked: boolean): string {
		if (isSel) return "pin-selected";
		if (isMarked) return "pin-marked";
		return "pin-plain";
	}
</script>

{#if page}
	<div class="pane-title @container gap-2">
		{#each crumbs() as c, i (i)}
			<button
				class="link link-hover font-mono normal-case"
				onclick={() =>
					assembly.openTable(
						page.doc,
						c.table,
						c.path,
						Number(c.label.match(/\[(\d+)\]/)?.[1]),
					)}>{c.label}</button
			>
			<span class="opacity-40">/</span>
		{/each}
		<span class="min-w-16 truncate font-mono normal-case" title={page.table}
			>{page.table.replace(/^DATA_SET_/, "")}</span
		>
		<span class="shrink-0 font-mono normal-case opacity-60 @max-xl:hidden"
			>{page.total} rows</span
		>
		<div class="flex-1"></div>
		{#if page.doc === "assembly" && page.path.length === 0}
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => ui.open("newContent")}
				title="Create a device, card, item or champion from a template row, with new ids and messages"
			>
				<Sparkles size={12} /><span class="@max-3xl:hidden">New content…</span>
			</button>
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => ui.open("vendor")}
				title="The Champion Vendor loot table decides which champions the client lists; add an existing champion to it"
			>
				<Users size={12} /><span class="@max-4xl:hidden">Champion Vendor…</span>
			</button>
		{/if}
		<button
			class="btn btn-ghost btn-xs"
			onclick={() => assembly.addRow(null)}
			title="Add an empty row at the end, with a fresh id ({keyLabel('Mod+Shift+Enter')})"
		>
			<Plus size={12} /><span class="@max-3xl:hidden">New row</span>
		</button>
		<button
			class="btn btn-ghost btn-xs"
			disabled={assembly.selected === null}
			onclick={() => assembly.selected !== null && assembly.addRow(assembly.selected)}
			title="Copy the selected row to the end with a fresh id and its own copies of its messages ({keyLabel(
				'Mod+D',
			)})"
		>
			<CopyPlus size={12} /><span class="@max-3xl:hidden">Duplicate</span>
		</button>
		<button
			class="btn btn-ghost btn-xs"
			disabled={assembly.targets().length === 0}
			onclick={deleteTargets}
			title="Delete the selected or marked rows (Delete key)"
		>
			<Trash2 size={12} />
			<span class="@max-3xl:hidden"
				>{assembly.marked.length > 1
					? `Delete ${assembly.marked.length}…`
					: "Delete…"}</span
			>
		</button>
		<details class="dropdown dropdown-end">
			<summary class="btn btn-ghost btn-xs list-none" title="Choose the columns shown">
				<Columns3 size={12} />
			</summary>
			<div
				class="dropdown-content bg-base-200 border-base-300 rounded-box z-40 mt-1 max-h-96 w-72 overflow-y-auto border p-2 shadow-xl"
			>
				<div class="mb-1 flex items-center gap-2 text-[11px] normal-case opacity-70">
					Columns of {page.table.replace(/^DATA_SET_/, "")}
					<span class="flex-1"></span>
					<button
						class="link"
						onclick={() => saveHidden({ ...hiddenAll, [page.table]: [] })}
						>show all</button
					>
				</div>
				{#each page.columns as c (c.name)}
					<label class="flex items-center gap-2 py-0.5 font-mono text-[11px] normal-case">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							checked={!hidden.has(c.name)}
							disabled={pinned.includes(c.name)}
							onchange={() => toggleColumn(c.name)}
						/>
						<span class="truncate">{c.name}</span>
						{#if pinned.includes(c.name)}<span class="opacity-50">pinned</span>{/if}
					</label>
				{/each}
			</div>
		</details>
	</div>
	<div class="border-base-300 flex items-center gap-1 border-b p-2">
		<input
			class="input input-sm min-w-0 flex-1 font-mono text-xs"
			placeholder="Filter rows (any cell contains…)"
			value={filterText}
			oninput={(e) => onFilter(e.currentTarget.value)}
		/>
		<span class="shrink-0 font-mono text-[11px] opacity-60">{from}–{to} of {page.total}</span>
		<button
			class="btn btn-ghost btn-xs"
			disabled={page.offset === 0}
			onclick={() => assembly.prevPage()}
			aria-label="Previous page"><ChevronLeft size={12} /></button
		>
		<button
			class="btn btn-ghost btn-xs"
			disabled={page.offset + assembly.limit >= page.total}
			onclick={() => assembly.nextPage()}
			aria-label="Next page"><ChevronRight size={12} /></button
		>
	</div>
	{#if assembly.marked.length > 1}
		<div
			class="bg-primary/10 border-base-300 flex items-center gap-2 border-b px-2 py-1 text-xs"
		>
			<span
				><b>{assembly.marked.length}</b> rows marked: editing a cell of one of them sets it on
				all.</span
			>
			<span class="flex-1"></span>
			<button class="btn btn-ghost btn-xs" onclick={copy}><Copy size={12} /> Copy</button>
			<button
				class="btn btn-ghost btn-xs"
				onclick={() =>
					assembly.copyTsv(
						assembly.targets(),
						columns.map((c) => c.name),
					)}>Copy as TSV</button
			>
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => (assembly.marked = [])}
				aria-label="Clear the marks"><X size={12} /></button
			>
		</div>
	{/if}
	<datalist id="valid-values">
		{#each validList as v (v.id)}
			<option value={v.id}>{v.text}</option>
		{/each}
	</datalist>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div
		class="min-h-0 flex-1 overflow-auto outline-none"
		role="grid"
		tabindex="0"
		aria-label="{page.table} rows"
		bind:this={gridEl}
		onkeydown={onKey}
	>
		<table class="table-xs table-pin-rows table w-max font-mono text-[11px]">
			<thead class="z-40">
				<tr>
					<th
						class="bg-base-200 sticky top-0 left-0 z-30 w-12 max-w-12 min-w-12 text-right"
						><span class="opacity-50">#</span></th
					>
					{#each columns as c, ci (c.name)}
						{@const left = stickyLeft(ci)}
						<th
							class="cursor-pointer whitespace-nowrap select-none {left !== null
								? 'bg-base-200 sticky top-0 z-30'
								: ''}"
							style:left={left !== null ? `${left}rem` : undefined}
							style:min-width={left !== null ? `${PIN_WIDTH[ci] ?? 8}rem` : undefined}
							style:max-width={left !== null ? `${PIN_WIDTH[ci] ?? 8}rem` : undefined}
							title="{c.kind}{c.count < page.total
								? ` · in ${c.count} rows`
								: ''} (click to sort)"
							onclick={() => assembly.setSort(c.name)}
						>
							<span class="inline-flex items-center gap-1">
								{c.name}
								{#if assembly.sort === c.name}
									{#if assembly.desc}<ArrowDown size={10} />{:else}<ArrowUp
											size={10}
										/>{/if}
								{/if}
							</span>
						</th>
					{/each}
				</tr>
			</thead>
			<tbody>
				{#each page.rows as [index, cells] (index)}
					{@const isSel = assembly.selected === index}
					{@const isMarked = markedSet.has(index)}
					<tr
						class="cursor-default {isSel
							? 'row-selected'
							: isMarked
								? 'bg-primary/10'
								: 'row-hover'}"
						data-row={index}
						onclick={(e) => onRowClick(e, index, null)}
						onauxclick={(e) => {
							if (e.button !== 1) return;
							tabs.openInNewTab();
							void assembly.select(index);
						}}
						oncontextmenu={(e) => onRowMenu(e, index)}
					>
						<td
							class="{pinBg(
								isSel,
								isMarked,
							)} sticky left-0 z-10 w-12 max-w-12 min-w-12 text-right"
							><span class={isMarked ? "text-primary" : "opacity-50"}>{index}</span
							></td
						>
						{#each columns as c, ci (c.name)}
							{@const cell = cells[c.name]}
							{@const h = hint(c, cell)}
							{@const left = stickyLeft(ci)}
							{@const active = isSel && col === c.name}
							<td
								data-col={c.name}
								class="max-w-[28rem] truncate {cell === undefined
									? 'opacity-30'
									: ''} {left !== null
									? `${pinBg(isSel, isMarked)} sticky z-10`
									: ''} {active
									? 'outline-primary outline-2 -outline-offset-2'
									: ''}"
								style:left={left !== null ? `${left}rem` : undefined}
								style:min-width={left !== null
									? `${PIN_WIDTH[ci] ?? 8}rem`
									: undefined}
								style:max-width={left !== null
									? `${PIN_WIDTH[ci] ?? 8}rem`
									: undefined}
								title={h ??
									(cell === undefined ? `no ${c.name} in this row` : undefined)}
								onclick={(e) => {
									e.stopPropagation();
									onRowClick(e, index, c.name);
								}}
								ondblclick={() => startEdit(index, c.name)}
							>
								{#if editing === `${index}|${c.name}`}
									<!-- svelte-ignore a11y_autofocus -->
									<input
										class="input input-xs w-56"
										autofocus
										list={c.name.endsWith("_VALUE_ID")
											? "valid-values"
											: undefined}
										value={editStart ?? text(cell)}
										onfocus={(e) => {
											if (editStart === null) e.currentTarget.select();
										}}
										onblur={(e) => {
											if (editing === `${index}|${c.name}`) {
												void commit(
													index,
													c.name,
													e.currentTarget.value,
													null,
												);
											}
										}}
										onkeydown={(e) => onEditKey(e, index, c.name)}
									/>
								{:else if cell !== null && typeof cell === "object"}
									<button
										class="link link-hover"
										onclick={(e) => {
											e.stopPropagation();
											void openNested(index, c);
										}}>{cell.rows} rows</button
									>
								{:else if isFlag(c.name) && (cell === "y" || cell === "n")}
									<input
										type="checkbox"
										class="toggle toggle-xs"
										checked={cell === "y"}
										onclick={(e) => {
											e.stopPropagation();
											e.preventDefault();
											void toggleFlag(index, c.name);
										}}
										aria-label={c.name}
									/>
								{:else if cell !== undefined && cell !== null && refOf(c.name)}
									<span class="inline-flex items-center gap-1">
										{text(cell)}
										<button
											class="opacity-40 hover:opacity-100"
											title="Go to {refOf(c.name)
												?.table} (Ctrl+click: new tab)"
											onclick={(e) => {
												e.stopPropagation();
												if (e.ctrlKey || e.metaKey) tabs.openInNewTab();
												void assembly.follow(c.name, cell);
											}}><ExternalLink size={10} /></button
										>
										{#if h}<span class="opacity-50">{h}</span>{/if}
									</span>
								{:else}
									{text(cell)}{#if h}<span class="ml-1 opacity-50">{h}</span>{/if}
								{/if}
							</td>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{:else}
	<div class="flex h-full items-center justify-center text-sm opacity-60">Pick a table.</div>
{/if}
