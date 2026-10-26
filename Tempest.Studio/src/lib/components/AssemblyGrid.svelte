<script lang="ts">
	import {
		ArrowDown,
		ArrowUp,
		ChevronLeft,
		ChevronRight,
		CopyPlus,
		ExternalLink,
		Sparkles,
		Trash2,
	} from "@lucide/svelte";
	import NewContentDialog from "$lib/components/NewContentDialog.svelte";
	import { assembly, isMessageColumn, refOf } from "$lib/studio/assembly.svelte";
	import { inField, reveal } from "$lib/studio/keys";
	import type { McsCell, McsColumn } from "../../../electron/src/ipc/contract";

	let filterText = $state("");
	let filterTimer: ReturnType<typeof setTimeout> | null = null;
	/** `${index}|${column}` of the cell being edited. */
	let editing = $state<string | null>(null);
	let confirmDelete = $state(false);
	let newOpen = $state(false);
	let newKind = $state<"device" | "card" | "item" | "champion">("device");

	const page = $derived(assembly.page);
	const from = $derived(page ? Math.min(page.offset + 1, page.total) : 0);
	const to = $derived(page ? Math.min(page.offset + page.rows.length, page.total) : 0);

	$effect(() => {
		// The filter box follows the store (cleared when another table opens).
		filterText = assembly.filter;
	});

	function onFilter(text: string) {
		filterText = text;
		if (filterTimer) clearTimeout(filterTimer);
		filterTimer = setTimeout(() => void assembly.setFilter(text), 250);
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

	async function commit(index: number, column: McsColumn, value: string) {
		editing = null;
		const current = text(page?.rows.find(([i]) => i === index)?.[1][column.name]);
		if (value === current) return;
		await assembly.setCell(index, column.name, value);
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

	let gridEl = $state<HTMLDivElement | null>(null);

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
		reveal(`[data-row="${row[0]}"]`, false);
	}

	async function onKey(event: KeyboardEvent) {
		if (!page || inField(event) || event.altKey || event.ctrlKey || event.metaKey) return;
		const at =
			assembly.selected === null ? -1 : page.rows.findIndex(([i]) => i === assembly.selected);
		const key = event.key;
		if (key === "ArrowDown") {
			event.preventDefault();
			await selectAt(at + 1);
		} else if (key === "ArrowUp") {
			event.preventDefault();
			await selectAt(at < 0 ? 0 : at - 1);
		} else if (key === "PageDown") {
			event.preventDefault();
			await assembly.nextPage();
			await selectAt(0);
		} else if (key === "PageUp") {
			event.preventDefault();
			await assembly.prevPage();
			await selectAt(0);
		} else if (key === "Home") {
			event.preventDefault();
			await selectAt(0);
		} else if (key === "End") {
			event.preventDefault();
			await selectAt(page.rows.length - 1);
		}
	}
</script>

{#if page}
	<div class="pane-title gap-2">
		{#each crumbs() as c (c.label)}
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
		<span class="truncate font-mono normal-case">{page.table.replace(/^DATA_SET_/, "")}</span>
		<span class="font-mono normal-case opacity-60">{page.total} rows</span>
		<div class="flex-1"></div>
		{#if page.doc === "assembly"}
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => {
					newKind =
						page.table === "DATA_SET_BOTS"
							? "champion"
							: page.table === "DATA_SET_ITEMS"
								? "item"
								: "device";
					newOpen = true;
				}}
				title="Create a device, card, item or champion from a template row, with new ids and messages"
			>
				<Sparkles size={12} /> New content…
			</button>
		{/if}
		<button
			class="btn btn-ghost btn-xs"
			onclick={() => assembly.insertRow(assembly.selected)}
			title={assembly.selected === null
				? "Add an empty row at the end"
				: "Add a copy of the selected row after it"}
		>
			<CopyPlus size={12} /> Add row
		</button>
		<button
			class="btn btn-xs {confirmDelete ? 'btn-error' : 'btn-ghost'}"
			disabled={assembly.selected === null}
			onblur={() => (confirmDelete = false)}
			onclick={() => {
				if (confirmDelete && assembly.selected !== null) {
					confirmDelete = false;
					void assembly.deleteRow(assembly.selected);
				} else {
					confirmDelete = true;
				}
			}}
			title="Delete the selected row"
		>
			<Trash2 size={12} />
			{confirmDelete ? "Click again" : "Delete"}
		</button>
	</div>
	<div class="border-base-300 flex items-center gap-1 border-b p-2">
		<input
			class="input input-sm flex-1 font-mono text-xs"
			placeholder="Filter rows (any cell contains…)"
			value={filterText}
			oninput={(e) => onFilter(e.currentTarget.value)}
		/>
		<span class="font-mono text-[11px] opacity-60">{from}–{to} of {page.total}</span>
		<button
			class="btn btn-ghost btn-xs"
			disabled={page.offset === 0}
			onclick={() => assembly.prevPage()}><ChevronLeft size={12} /></button
		>
		<button
			class="btn btn-ghost btn-xs"
			disabled={page.offset + assembly.limit >= page.total}
			onclick={() => assembly.nextPage()}><ChevronRight size={12} /></button
		>
	</div>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div
		class="min-h-0 flex-1 overflow-auto outline-none"
		role="grid"
		tabindex="0"
		bind:this={gridEl}
		onkeydown={onKey}
	>
		<table class="table-xs table-pin-rows table w-max font-mono text-[11px]">
			<thead>
				<tr>
					<th class="w-10 text-right opacity-50">#</th>
					{#each page.columns as c (c.name)}
						<th
							class="cursor-pointer whitespace-nowrap select-none"
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
					<tr
						class="cursor-default {assembly.selected === index
							? 'row-selected'
							: 'row-hover'}"
						data-row={index}
						onclick={() => {
							gridEl?.focus({ preventScroll: true });
							void assembly.select(index);
						}}
					>
						<td class="text-right opacity-50">{index}</td>
						{#each page.columns as c (c.name)}
							{@const cell = cells[c.name]}
							{@const h = hint(c, cell)}
							<td
								class="max-w-[28rem] truncate {cell === undefined
									? 'opacity-30'
									: ''}"
								title={h ??
									(cell === undefined ? `no ${c.name} in this row` : undefined)}
								ondblclick={() => {
									if (editable(c)) editing = `${index}|${c.name}`;
								}}
							>
								{#if editing === `${index}|${c.name}`}
									<!-- svelte-ignore a11y_autofocus -->
									<input
										class="input input-xs w-48"
										autofocus
										value={text(cell)}
										onblur={(e) => commit(index, c, e.currentTarget.value)}
										onkeydown={(e) => {
											if (e.key === "Enter") e.currentTarget.blur();
											if (e.key === "Escape") editing = null;
										}}
									/>
								{:else if cell !== null && typeof cell === "object"}
									<button
										class="link link-hover"
										onclick={(e) => {
											e.stopPropagation();
											void openNested(index, c);
										}}>{cell.rows} rows</button
									>
								{:else if cell !== undefined && cell !== null && refOf(c.name)}
									<span class="inline-flex items-center gap-1">
										{text(cell)}
										<button
											class="opacity-40 hover:opacity-100"
											title="Go to {refOf(c.name)?.table}"
											onclick={(e) => {
												e.stopPropagation();
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

<NewContentDialog bind:open={newOpen} bind:kind={newKind} />
