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
		Shirt,
		Users,
		X,
	} from "@lucide/svelte";
	import { assembly, isMessageColumn, refOf, type Ref } from "$lib/studio/assembly.svelte";
	import { keyLabel } from "$lib/studio/commands.svelte";
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

	/** What every cell of a column needs, worked out once per table instead of per cell. */
	type Col = McsColumn & {
		ref: Ref | null;
		msg: boolean;
		flag: boolean;
		valueId: boolean;
		editable: boolean;
		left: number | null;
		width: number | null;
	};
	const cols = $derived<Col[]>(
		columns.map((c, ci) => {
			const left = stickyLeft(ci);
			return {
				...c,
				ref: refOf(c.name),
				msg: isMessageColumn(c.name),
				flag: isFlag(c.name),
				valueId: c.name.endsWith("_VALUE_ID"),
				editable: editable(c),
				left,
				width: left !== null ? (PIN_WIDTH[ci] ?? 8) : null,
			};
		}),
	);
	const colByName = $derived(new Map(cols.map((c) => [c.name, c])));

	/**
	 * Width (in characters) each column needs for the widest cell of the whole page: only
	 * the rows in view are rendered, and auto-sized columns would jump while scrolling.
	 */
	const colChars = $derived.by(() => {
		const out = new Map<string, number>();
		if (!page) return out;
		for (const c of cols) {
			let w = 0;
			for (const [, cells] of page.rows) {
				const cell = cells[c.name];
				if (cell === undefined || cell === null) continue;
				let n: number;
				if (typeof cell === "object") n = String(cell.rows).length + 5;
				else if (c.flag && (cell === "y" || cell === "n")) n = 4;
				else {
					n = String(cell).length;
					const h = hint(c, cell);
					if (h) n += h.length + 1;
					if (c.ref) n += 3;
				}
				if (n > w) w = n;
			}
			out.set(c.name, w);
		}
		return out;
	});

	// ----- virtual rows: only the rows in view (plus a margin) are in the DOM

	/** Height of a body row in px, measured from the first rendered row. */
	let rowH = $state(25);
	const OVERSCAN = 10;
	let scrollTop = $state(0);
	let viewH = $state(600);
	let headEl = $state<HTMLTableSectionElement | null>(null);
	const allRows = $derived(page?.rows ?? []);
	/** Position of a row (by index) in the page. */
	const posOf = $derived(new Map(allRows.map(([i], k) => [i, k])));
	const firstRow = $derived(Math.max(0, Math.floor(scrollTop / rowH) - OVERSCAN));
	const lastRow = $derived(
		Math.min(allRows.length, Math.ceil((scrollTop + viewH) / rowH) + OVERSCAN),
	);
	const visibleRows = $derived(allRows.slice(firstRow, lastRow));

	// ----- virtual columns: the unpinned columns outside the view are spacers
	// (tables have up to a hundred columns, and a dozen fit on screen)

	let scrollLeft = $state(0);
	let viewW = $state(1200);
	/** Pixels of a `ch` of the grid font and of a `rem` (zoom and fonts decide them). */
	let chPx = $state(6.6);
	/** The same for header text (its own font size and weight). */
	let headChPx = $state(7.5);
	let remPx = $state(16);
	/** The exact width of every column in px (set on its header, so the table follows). */
	const colPx = $derived(
		new Map(
			cols.map((c) => [
				c.name,
				c.width !== null
					? c.width * remPx
					: Math.min(
							Math.max(
								(colChars.get(c.name) ?? 0) * chPx,
								(c.name.length + (assembly.sort === c.name ? 2 : 0)) * headChPx,
							) + remPx,
							28 * remPx,
						),
			]),
		),
	);
	const pinnedCols = $derived(cols.filter((c) => c.left !== null));
	/** Unpinned columns with their left edge in table coordinates. */
	const flowCols = $derived.by(() => {
		let x = 3 * remPx + pinnedCols.reduce((w, c) => w + (colPx.get(c.name) ?? 0), 0);
		return cols
			.filter((c) => c.left === null)
			.map((c) => {
				const w = colPx.get(c.name) ?? 0;
				const at = { col: c, x, w };
				x += w;
				return at;
			});
	});
	const COL_OVERSCAN = 2;
	const colWindow = $derived.by(() => {
		const lo = scrollLeft;
		const hi = scrollLeft + viewW;
		let first = flowCols.findIndex((f) => f.x + f.w >= lo);
		if (first < 0) first = flowCols.length;
		let last = first;
		while (last < flowCols.length && flowCols[last].x <= hi) last++;
		first = Math.max(0, first - COL_OVERSCAN);
		last = Math.min(flowCols.length, last + COL_OVERSCAN);
		const start = flowCols[first]?.x ?? 0;
		const tail = flowCols.at(-1);
		const end = tail ? tail.x + tail.w : 0;
		const lastEnd = last > first ? flowCols[last - 1].x + flowCols[last - 1].w : start;
		return {
			before: first > 0 ? start - flowCols[0].x : 0,
			after: Math.max(0, end - lastEnd),
			cols: flowCols.slice(first, last).map((f) => f.col),
		};
	});
	const shownCols = $derived([...pinnedCols, ...colWindow.cols]);

	$effect(() => {
		// Measure the font once the grid is on screen (and again when the zoom changes).
		void viewW;
		if (!gridEl) return;
		const probe = document.createElement("span");
		probe.className = "font-mono text-[11px]";
		probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre";
		probe.textContent = "0".repeat(100);
		gridEl.append(probe);
		const w = probe.getBoundingClientRect().width / 100;
		probe.remove();
		const th = headEl?.querySelector("th");
		if (th) {
			const hp = document.createElement("span");
			hp.style.cssText = "position:absolute;visibility:hidden;white-space:pre";
			hp.textContent = "M".repeat(100);
			th.append(hp);
			const hw = hp.getBoundingClientRect().width / 100;
			hp.remove();
			if (hw > 0 && Math.abs(hw - headChPx) > 0.01) headChPx = hw;
		}
		const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
		if (w > 0 && Math.abs(w - chPx) > 0.01) chPx = w;
		if (Math.abs(rem - remPx) > 0.01) remPx = rem;
	});

	let scrollFrame = 0;
	function onScroll() {
		if (scrollFrame) return;
		scrollFrame = requestAnimationFrame(() => {
			scrollFrame = 0;
			if (!gridEl) return;
			scrollTop = gridEl.scrollTop;
			scrollLeft = gridEl.scrollLeft;
		});
	}

	// Another table, filter, sort or page starts at the top.
	const pageKey = $derived(
		page
			? `${page.doc}|${page.table}|${JSON.stringify(page.path)}|${page.offset}|${assembly.filter}|${assembly.sort}|${assembly.desc}`
			: "",
	);
	let lastPageKey = "";
	$effect(() => {
		const key = pageKey;
		if (key === lastPageKey) return;
		lastPageKey = key;
		if (gridEl) gridEl.scrollTop = 0;
		scrollTop = 0;
	});

	$effect(() => {
		// Measure the row height once rows are on screen (fonts and zoom decide it).
		void visibleRows.length;
		requestAnimationFrame(() => {
			const tr = gridEl?.querySelector<HTMLElement>("tbody tr[data-row]");
			const h = tr?.getBoundingClientRect().height;
			if (h && Math.abs(h - rowH) > 0.5) rowH = h;
		});
	});

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

	function hint(column: Col, cell: McsCell | undefined): string | null {
		if (cell === undefined || cell === null || typeof cell === "object") return null;
		if (column.msg) return assembly.message(cell);
		if (column.valueId && assembly.validValues) {
			return assembly.validValues[String(cell)]?.text ?? null;
		}
		return null;
	}

	function editable(column: McsColumn): boolean {
		return column.kind !== "rowset" && column.kind !== "blob";
	}

	const isFlag = (name: string) => name.endsWith("_FLAG");

	function cellOf(index: number, name: string): McsCell | undefined {
		const pos = posOf.get(index);
		return pos === undefined ? undefined : page?.rows[pos]?.[1][name];
	}

	/** Valid values for a `*_VALUE_ID` editor: id → text (only built while one is edited). */
	const editingValueId = $derived(editing?.endsWith("_VALUE_ID") ?? false);
	const validList = $derived(
		editingValueId && assembly.validValues
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
		return posOf.get(index) ?? -1;
	}

	/** A page change is under way (held keys must not skip pages). */
	let flipping = false;

	async function selectAt(pos: number) {
		if (!page) return;
		let target = pos;
		if (target < 0 || target >= page.rows.length) {
			if (flipping) return;
			flipping = true;
			try {
				if (target < 0) {
					if (page.offset === 0) return;
					await assembly.prevPage();
					target = (assembly.page?.rows.length ?? 1) - 1;
				} else {
					if (page.offset + assembly.limit >= page.total) return;
					await assembly.nextPage();
					target = 0;
				}
			} finally {
				flipping = false;
			}
		}
		const row = assembly.page?.rows[target];
		if (!row) return;
		void assembly.select(row[0]);
		revealCell();
	}

	function moveCol(step: number) {
		const i = columns.findIndex((c) => c.name === col);
		const next = columns[Math.max(0, Math.min(columns.length - 1, i + step))];
		if (next) col = next.name;
		revealCell();
	}

	/** Scroll the cell cursor into view: rows by arithmetic (they may not be rendered yet). */
	function revealCell() {
		const sel = assembly.selected;
		const el0 = gridEl;
		if (sel === null || !el0) return;
		const pos = posOf.get(sel);
		if (pos !== undefined) {
			const head = headEl?.offsetHeight ?? 0;
			const top = pos * rowH;
			const h = el0.clientHeight - head;
			if (top < el0.scrollTop) el0.scrollTop = top;
			else if (top + rowH > el0.scrollTop + h) el0.scrollTop = top + rowH - h;
			scrollTop = el0.scrollTop;
		}
		// Columns by arithmetic too; pinned columns cover the left edge.
		const f = flowCols.find((x) => x.col.name === col);
		if (f) {
			const pinnedW = flowCols[0]?.x ?? 0;
			if (f.x < el0.scrollLeft + pinnedW) el0.scrollLeft = f.x - pinnedW;
			else if (f.x + f.w > el0.scrollLeft + el0.clientWidth) {
				el0.scrollLeft = f.x + f.w - el0.clientWidth;
			}
			scrollLeft = el0.scrollLeft;
		}
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
		// AltGr arrives as Ctrl+Alt on Windows: it types characters (@, {, €…).
		const altGr = event.getModifierState?.("AltGraph") ?? false;
		if ((event.ctrlKey || event.metaKey) && !altGr) {
			await onModKey(event, page.rows);
			return;
		}
		if (event.altKey && !altGr) return;
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
				// A screenful of rows; past the ends, the next or previous page.
				event.preventDefault();
				const n = Math.max(
					1,
					Math.floor(
						((gridEl?.clientHeight ?? 600) - (headEl?.offsetHeight ?? 0)) / rowH,
					) - 1,
				);
				const down = key === "PageDown";
				const target = down
					? at >= page.rows.length - 1
						? at + 1
						: Math.min(page.rows.length - 1, at + n)
					: at <= 0
						? -1
						: Math.max(0, at - n);
				assembly.marked = [];
				await selectAt(target);
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

	// ----- one set of handlers for the whole body (no closures per cell)

	type Hit = { index: number; col: string | null; action: string | null };
	function hitOf(event: Event): Hit | null {
		const t = event.target as Element | null;
		const tr = t?.closest<HTMLElement>("tr[data-row]");
		if (!t || !tr) return null;
		const td = t.closest<HTMLElement>("td[data-col]");
		return {
			index: Number(tr.dataset.row),
			col: td?.dataset.col ?? null,
			action: t.closest("[data-action]")?.getAttribute("data-action") ?? null,
		};
	}

	/** Clicks inside the cell editor stay there (moving the caret must not commit it). */
	function inEditor(event: Event): boolean {
		return !!(event.target as Element | null)?.closest(".cell-editor");
	}

	function onBodyClick(event: MouseEvent) {
		if (inEditor(event)) return;
		const hit = hitOf(event);
		if (!hit) return;
		const column = hit.col ? colByName.get(hit.col) : undefined;
		if (hit.action && column) {
			event.stopPropagation();
			if (hit.action === "flag") {
				event.preventDefault();
				void toggleFlag(hit.index, column.name);
				return;
			}
			if (hit.action === "nested") {
				void openNested(hit.index, column);
				return;
			}
			if (hit.action === "follow") {
				const cell = cellOf(hit.index, column.name);
				if (event.ctrlKey || event.metaKey) tabs.openInNewTab();
				void assembly.follow(column.name, cell ?? null);
				return;
			}
		}
		onRowClick(event, hit.index, hit.col);
	}

	function onBodyDblClick(event: MouseEvent) {
		if (inEditor(event)) return;
		const hit = hitOf(event);
		if (hit?.col && !hit.action) startEdit(hit.index, hit.col);
	}

	function onBodyAuxClick(event: MouseEvent) {
		if (event.button !== 1) return;
		const hit = hitOf(event);
		if (!hit) return;
		tabs.openInNewTab();
		void assembly.select(hit.index);
	}

	function onBodyMenu(event: MouseEvent) {
		if (inEditor(event)) return;
		const hit = hitOf(event);
		if (hit) onRowMenu(event, hit.index);
	}

	/** Opaque background of a pinned cell, matching its row's highlight. */
	function pinBg(isSel: boolean, isMarked: boolean): string {
		if (isSel) return "pin-selected";
		if (isMarked) return "pin-marked";
		return "pin-plain";
	}
</script>

{#snippet head(c: Col)}
	{@const w = `${colPx.get(c.name) ?? 0}px`}
	<th
		class="cursor-pointer overflow-hidden text-ellipsis whitespace-nowrap select-none {c.left !==
		null
			? 'pin bg-base-200 sticky top-0 z-30'
			: ''}"
		style:left={c.left !== null ? `${c.left}rem` : undefined}
		style:width={w}
		style:min-width={w}
		style:max-width={w}
		title="{c.kind}{page && c.count < page.total
			? ` · in ${c.count} rows`
			: ''} (click to sort)"
		onclick={() => assembly.setSort(c.name)}
	>
		<span class="inline-flex items-center gap-1">
			{c.name}
			{#if assembly.sort === c.name}
				{#if assembly.desc}<ArrowDown size={10} />{:else}<ArrowUp size={10} />{/if}
			{/if}
		</span>
	</th>
{/snippet}

{#snippet cellOfRow(
	c: Col,
	index: number,
	cells: Record<string, McsCell>,
	isSel: boolean,
	pin: string,
)}
	{@const cell = cells[c.name]}
	{@const h = hint(c, cell)}
	<td
		data-col={c.name}
		class="max-w-[28rem] truncate {cell === undefined ? 'opacity-30' : ''} {c.left !== null
			? `${pin} sticky z-10`
			: ''} {isSel && col === c.name ? 'outline-primary outline-2 -outline-offset-2' : ''}"
		style:left={c.left !== null ? `${c.left}rem` : undefined}
		style:min-width={c.width !== null ? `${c.width}rem` : undefined}
		style:max-width={c.width !== null ? `${c.width}rem` : undefined}
		title={h ?? (cell === undefined ? `no ${c.name} in this row` : undefined)}
	>
		{#if editing === `${index}|${c.name}`}
			<!-- svelte-ignore a11y_autofocus -->
			<input
				class="cell-editor input input-xs w-56"
				autofocus
				list={c.valueId ? "valid-values" : undefined}
				value={editStart ?? text(cell)}
				onfocus={(e) => {
					if (editStart === null) e.currentTarget.select();
				}}
				onblur={(e) => {
					if (editing === `${index}|${c.name}`) {
						void commit(index, c.name, e.currentTarget.value, null);
					}
				}}
				onkeydown={(e) => onEditKey(e, index, c.name)}
			/>
		{:else if cell !== null && typeof cell === "object"}
			<button class="link link-hover" data-action="nested">{cell.rows} rows</button>
		{:else if c.flag && (cell === "y" || cell === "n")}
			<input
				type="checkbox"
				class="toggle toggle-xs"
				checked={cell === "y"}
				data-action="flag"
				aria-label={c.name}
			/>
		{:else if cell !== undefined && cell !== null && c.ref}
			<span class="inline-flex items-center gap-1">
				{text(cell)}
				<button
					class="opacity-40 hover:opacity-100"
					title="Go to {c.ref.table} (Ctrl+click: new tab)"
					data-action="follow"><ExternalLink size={10} /></button
				>
				{#if h}<span class="opacity-50">{h}</span>{/if}
			</span>
		{:else}
			{text(cell)}{#if h}<span class="ml-1 opacity-50">{h}</span>{/if}
		{/if}
	</td>
{/snippet}

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
				title="Create a device, card, item, skin or champion from a template row, with new ids and messages"
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
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => ui.open("skins")}
				title="Skin collections: the loot table the client lists each champion's skins from; copy a collection or list a skin"
			>
				<Shirt size={12} /><span class="@max-4xl:hidden">Skins…</span>
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
	{#if editingValueId}
		<datalist id="valid-values">
			{#each validList as v (v.id)}
				<option value={v.id}>{v.text}</option>
			{/each}
		</datalist>
	{/if}
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div
		class="min-h-0 flex-1 overflow-auto outline-none"
		role="grid"
		tabindex="0"
		aria-label="{page.table} rows"
		aria-rowcount={allRows.length}
		bind:this={gridEl}
		bind:clientHeight={viewH}
		bind:clientWidth={viewW}
		onscroll={onScroll}
		onkeydown={onKey}
	>
		<table class="asm-grid table-xs table-pin-rows table w-max font-mono text-[11px]">
			<thead class="z-40" bind:this={headEl}>
				<tr>
					<th
						class="pin bg-base-200 sticky top-0 left-0 z-30 w-12 max-w-12 min-w-12 text-right"
						><span class="opacity-50">#</span></th
					>
					{#each pinnedCols as c (c.name)}
						{@render head(c)}
					{/each}
					{#if colWindow.before > 0}
						<th class="asm-gap" style:width="{colWindow.before}px"></th>
					{/if}
					{#each colWindow.cols as c (c.name)}
						{@render head(c)}
					{/each}
					{#if colWindow.after > 0}
						<th class="asm-gap" style:width="{colWindow.after}px"></th>
					{/if}
				</tr>
			</thead>
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<tbody
				onclick={onBodyClick}
				ondblclick={onBodyDblClick}
				onauxclick={onBodyAuxClick}
				oncontextmenu={onBodyMenu}
			>
				{#if firstRow > 0}
					<tr class="asm-spacer" style:height="{firstRow * rowH}px"><td></td></tr>
				{/if}
				{#each visibleRows as [index, cells] (index)}
					{@const isSel = assembly.selected === index}
					{@const isMarked = markedSet.has(index)}
					{@const pin = pinBg(isSel, isMarked)}
					<tr
						class="cursor-default {isSel
							? 'row-selected'
							: isMarked
								? 'bg-primary/10'
								: 'row-hover'}"
						data-row={index}
					>
						<td class="{pin} sticky left-0 z-10 w-12 max-w-12 min-w-12 text-right"
							><span class={isMarked ? "text-primary" : "opacity-50"}>{index}</span
							></td
						>
						{#each pinnedCols as c (c.name)}
							{@render cellOfRow(c, index, cells, isSel, pin)}
						{/each}
						{#if colWindow.before > 0}<td class="asm-gap"></td>{/if}
						{#each colWindow.cols as c (c.name)}
							{@render cellOfRow(c, index, cells, isSel, pin)}
						{/each}
						{#if colWindow.after > 0}<td class="asm-gap"></td>{/if}
					</tr>
				{/each}
				{#if lastRow < allRows.length}
					<tr class="asm-spacer" style:height="{(allRows.length - lastRow) * rowH}px"
						><td></td></tr
					>
				{/if}
			</tbody>
		</table>
	</div>
{:else}
	<div class="flex h-full items-center justify-center text-sm opacity-60">Pick a table.</div>
{/if}
