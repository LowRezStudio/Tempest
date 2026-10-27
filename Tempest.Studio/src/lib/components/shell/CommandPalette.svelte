<!--
	One field for everything: plain text finds packages and assembly tables,
	`>` runs a command, `@` finds an object of the open package (or filters the
	assembly table). Enter opens, Ctrl+Enter opens in a new tab.
-->
<script lang="ts">
	import { Code2, Database, FileBox, Map as MapIcon, Search, Table } from "@lucide/svelte";
	import { tick } from "svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { commands, keyLabel } from "$lib/studio/commands.svelte";
	import { fuzzy } from "$lib/studio/fuzzy";
	import { studio } from "$lib/studio/state.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { Component } from "svelte";

	type Item = {
		key: string;
		label: string;
		detail?: string;
		group: string;
		keys?: string;
		icon?: Component;
		disabled?: boolean;
		score: number;
		run: (newTab: boolean) => unknown;
	};

	let text = $state("");
	let active = $state(0);
	let input = $state<HTMLInputElement | null>(null);
	let list = $state<HTMLDivElement | null>(null);
	const LIMIT = 80;

	$effect(() => {
		const mode = ui.palette;
		if (!mode) return;
		const prefix = mode === "commands" ? ">" : mode === "objects" ? "@" : "";
		text = prefix + ui.paletteQuery;
		active = 0;
		void tick().then(() => {
			input?.focus();
			input?.setSelectionRange(text.length, text.length);
		});
	});

	const kind = $derived(
		text.startsWith(">") ? "commands" : text.startsWith("@") ? "objects" : "open",
	);
	const query = $derived(kind === "open" ? text : text.slice(1));

	function ranked(items: Omit<Item, "score">[], by: (i: Omit<Item, "score">) => string): Item[] {
		const out: Item[] = [];
		for (const i of items) {
			const s = fuzzy(query, by(i));
			if (s !== null) out.push({ ...i, score: s });
		}
		return query.trim() ? out.sort((a, b) => b.score - a.score) : out;
	}

	const items = $derived.by<Item[]>(() => {
		if (kind === "commands") {
			return ranked(
				commands.list
					.filter((c) => !c.hidden)
					.map((c) => ({
						key: c.id,
						label: c.title,
						group: c.group,
						keys: c.keys,
						icon: c.icon,
						disabled: !commands.enabled(c),
						run: () => commands.run(c.id),
					})),
				(i) => `${i.group} ${i.label}`,
			).slice(0, LIMIT);
		}
		if (kind === "objects") {
			const out: Omit<Item, "score">[] = [];
			if (studio.mode === "assembly" && assembly.page) {
				const p = assembly.page;
				out.push({
					key: "filter",
					label: query.trim()
						? `Filter ${p.table.replace(/^DATA_SET_/, "")} rows containing "${query.trim()}"`
						: `Clear the row filter of ${p.table.replace(/^DATA_SET_/, "")}`,
					group: "Assembly",
					icon: Table,
					run: () => assembly.setFilter(query.trim()),
				});
				return out.map((i) => ({ ...i, score: 0 }));
			}
			if (studio.pkg) {
				for (const r of studio.pkg.exports) {
					out.push({
						key: `e${r.i}`,
						label: r.path,
						detail: r.class,
						group: studio.pkg.name,
						icon: FileBox,
						run: (newTab) => {
							if (newTab) tabs.openInNewTab();
							return studio.select(r.i);
						},
					});
				}
			}
			const hits = ranked(out, (i) => i.label).slice(0, LIMIT);
			if (query.trim() && studio.workspace) {
				hits.push({
					key: "search",
					label: `Search every package for "${query.trim()}"`,
					group: "Search",
					icon: Search,
					score: 0,
					run: () => {
						ui.searchQuery = query.trim();
						ui.open("search");
					},
				});
			}
			return hits;
		}
		const out: Omit<Item, "score">[] = [];
		for (const p of studio.packages) {
			out.push({
				key: `p${p.path}`,
				label: p.name,
				detail: p.size > 0 ? undefined : "outside the game folder",
				group: "Packages",
				icon: p.script ? Code2 : p.map ? MapIcon : FileBox,
				run: async (newTab) => {
					if (newTab) tabs.openInNewTab();
					studio.mode = "packages";
					await studio.openPackage(p.path);
				},
			});
		}
		for (const d of assembly.summary?.docs ?? []) {
			for (const t of d.tables) {
				out.push({
					key: `t${d.id}|${t.name}`,
					label: t.name.replace(/^DATA_SET_/, ""),
					detail: `${d.id} · ${t.rows} rows`,
					group: "Assembly tables",
					icon: Database,
					run: async (newTab) => {
						if (newTab) tabs.openInNewTab();
						studio.mode = "assembly";
						await assembly.openTable(d.id, t.name, []);
					},
				});
			}
		}
		return ranked(out, (i) => i.label).slice(0, LIMIT);
	});

	$effect(() => {
		void items;
		active = Math.min(active, Math.max(0, items.length - 1));
	});

	function close() {
		ui.palette = null;
		ui.paletteQuery = "";
	}

	function pick(i: number, newTab = false) {
		const item = items[i];
		if (!item || item.disabled) return;
		close();
		void item.run(newTab);
	}

	function onKey(event: KeyboardEvent) {
		if (event.key === "Escape") {
			event.preventDefault();
			close();
		} else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			const n = items.length;
			if (n === 0) return;
			active = (active + (event.key === "ArrowDown" ? 1 : -1) + n) % n;
			list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
		} else if (event.key === "PageDown" || event.key === "PageUp") {
			event.preventDefault();
			active = Math.max(
				0,
				Math.min(items.length - 1, active + (event.key === "PageDown" ? 10 : -10)),
			);
			list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
		} else if (event.key === "Enter") {
			event.preventDefault();
			pick(active, event.ctrlKey || event.metaKey);
		}
	}

	const placeholder = $derived(
		kind === "commands"
			? "Run a command…"
			: kind === "objects"
				? studio.mode === "assembly"
					? "Filter the open table's rows…"
					: "Find an object of the open package…"
				: "Go to a package or table (> commands, @ objects)…",
	);
</script>

{#if ui.palette}
	<div
		class="fixed inset-0 z-50 flex justify-center bg-black/30 pt-[12vh]"
		role="presentation"
		onmousedown={(e) => e.target === e.currentTarget && close()}
	>
		<div
			class="bg-base-200 border-base-300 flex h-fit max-h-[70vh] w-[min(42rem,92vw)] flex-col overflow-hidden rounded-lg border shadow-2xl"
			role="dialog"
			aria-modal="true"
			aria-label="Command palette"
		>
			<div class="border-base-300 flex items-center gap-2 border-b px-3">
				<Search size={15} class="opacity-60" />
				<input
					bind:this={input}
					bind:value={text}
					class="h-11 flex-1 bg-transparent text-sm outline-none"
					{placeholder}
					onkeydown={onKey}
					oninput={() => (active = 0)}
					spellcheck="false"
					autocomplete="off"
					aria-controls="palette-list"
				/>
			</div>
			<div
				id="palette-list"
				class="min-h-0 overflow-y-auto py-1"
				bind:this={list}
				role="listbox"
			>
				{#each items as item, i (item.key)}
					{@const prev = items[i - 1]}
					{#if !prev || prev.group !== item.group}
						<div
							class="px-3 pt-2 pb-1 text-[10px] font-semibold tracking-wide uppercase opacity-50"
						>
							{item.group}
						</div>
					{/if}
					<button
						type="button"
						data-i={i}
						class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm {i ===
						active
							? 'bg-primary/15'
							: ''} {item.disabled ? 'opacity-40' : ''}"
						role="option"
						aria-selected={i === active}
						onmousemove={() => (active = i)}
						onclick={(e) => pick(i, e.ctrlKey || e.metaKey)}
					>
						{#if item.icon}<item.icon size={14} class="shrink-0 opacity-70" />{/if}
						<span class="truncate {kind === 'commands' ? '' : 'font-mono text-xs'}"
							>{item.label}</span
						>
						{#if item.detail}<span class="truncate text-xs opacity-50"
								>{item.detail}</span
							>{/if}
						<span class="flex-1"></span>
						{#if item.keys}<kbd class="kbd kbd-xs opacity-70">{keyLabel(item.keys)}</kbd
							>{/if}
					</button>
				{:else}
					<div class="px-3 py-3 text-sm opacity-60">Nothing matches.</div>
				{/each}
			</div>
			<div class="border-base-300 flex gap-3 border-t px-3 py-1.5 text-[11px] opacity-60">
				<span><kbd class="kbd kbd-xs">↑↓</kbd> move</span>
				<span><kbd class="kbd kbd-xs">Enter</kbd> open</span>
				<span
					><kbd class="kbd kbd-xs">{keyLabel("Mod+Enter").replace("Enter", "↵")}</kbd> new tab</span
				>
				<span class="flex-1"></span>
				<span><b>&gt;</b> commands · <b>@</b> objects</span>
			</div>
		</div>
	</div>
{/if}
