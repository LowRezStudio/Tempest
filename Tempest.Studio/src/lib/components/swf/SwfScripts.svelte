<!--
	Every ActionScript 2 script of a movie: class definitions (DoInitAction),
	frame scripts of the root and of sprites (DoAction), clip event handlers
	(onClipEvent on placements) and button actions, with the selected one in
	the script editor (decompiled ActionScript or P-code).
-->
<script lang="ts">
	import SwfScriptEditor from "$lib/components/swf/SwfScriptEditor.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfScript } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
	}
	const { doc }: Props = $props();

	let filter = $state("");

	const GROUPS: { kind: SwfScript["kind"]; title: string }[] = [
		{ kind: "DoInitAction", title: "Classes" },
		{ kind: "DoAction", title: "Frame scripts" },
		{ kind: "ClipAction", title: "Clip events" },
		{ kind: "ButtonAction", title: "Buttons" },
	];

	function title(sc: SwfScript): string {
		const where = sc.owner === 0 ? "root" : doc.label(sc.owner);
		switch (sc.kind) {
			case "DoInitAction": {
				const name = sc.sprite === null ? undefined : doc.chars.get(sc.sprite)?.names[0];
				return name ? name.replace(/^__Packages\./, "") : `#initclip ${sc.label}`;
			}
			case "DoAction": {
				return `${where} · frame ${sc.frame + 1}`;
			}
			case "ClipAction": {
				const p = sc.placed;
				const target = p?.name ?? (p?.character == null ? "" : doc.label(p.character));
				return `onClipEvent(${sc.label}) · ${target || `depth ${p?.depth}`} in ${where}`;
			}
			case "ButtonAction": {
				return `on(${sc.label}) · button ${sc.tag}`;
			}
		}
	}

	const scripts = $derived(doc.info?.scripts ?? []);
	const visible = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return scripts.filter(
			(sc) => q === "" || title(sc).toLowerCase().includes(q) || sc.key === q,
		);
	});
	const groups = $derived(
		GROUPS.map((g) => ({ ...g, items: visible.filter((sc) => sc.kind === g.kind) })).filter(
			(g) => g.items.length > 0,
		),
	);
	const selected = $derived(scripts.find((sc) => sc.key === doc.script) ?? null);

	function onkeydown(e: KeyboardEvent) {
		if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
		const flat = groups.flatMap((g) => g.items);
		if (flat.length === 0) return;
		const at = flat.findIndex((sc) => sc.key === doc.script);
		const next =
			e.key === "ArrowDown" ? Math.min(flat.length - 1, at + 1) : Math.max(0, at - 1);
		doc.script = flat[next].key;
		e.preventDefault();
		document
			.querySelector(`[data-swf-script="${CSS.escape(flat[next].key)}"]`)
			?.scrollIntoView({ block: "nearest" });
	}
</script>

<div class="flex h-full min-h-0">
	<div class="border-base-300 flex w-72 shrink-0 flex-col border-r">
		<input
			class="input input-xs m-1 font-mono"
			placeholder="Filter scripts…"
			bind:value={filter}
		/>
		<div
			class="min-h-0 flex-1 overflow-auto pb-1 text-xs"
			role="listbox"
			tabindex="0"
			{onkeydown}
		>
			{#if scripts.length === 0}
				<p class="p-2 opacity-60">This movie has no scripts.</p>
			{/if}
			{#each groups as g (g.kind)}
				<div
					class="bg-base-200 sticky top-0 px-2 py-0.5 text-[10px] font-semibold uppercase opacity-70"
				>
					{g.title} <span class="opacity-60">{g.items.length}</span>
				</div>
				{#each g.items as sc (sc.key)}
					<button
						class="hover:bg-base-300 flex w-full items-center gap-1 px-2 py-0.5 text-left font-mono {doc.script ===
						sc.key
							? 'bg-primary/20'
							: ''}"
						data-swf-script={sc.key}
						role="option"
						aria-selected={doc.script === sc.key}
						onclick={() => (doc.script = sc.key)}
						title={`${sc.kind} ${sc.key}, ${sc.bytes} bytes`}
					>
						<span class="min-w-0 flex-1 truncate">{title(sc)}</span>
						{#if doc.scriptEdit(sc.loc)}<span
								class="bg-warning size-1.5 shrink-0 rounded-full"
							></span>{/if}
					</button>
				{/each}
			{/each}
		</div>
	</div>
	<div class="min-w-0 flex-1">
		{#if selected}
			{#key selected.key}
				<SwfScriptEditor {doc} script={selected} title={title(selected)} />
			{/key}
		{:else}
			<p class="p-3 text-xs opacity-60">
				Pick a script. Classes read as ActionScript 2 <code>class</code> definitions; every script
				can be edited as ActionScript or as P-code (the bytecode, one action per line), checked,
				and staged as a pending edit that Apply writes into the package.
			</p>
		{/if}
	</div>
</div>
