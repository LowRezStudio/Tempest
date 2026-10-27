<!--
	The movie editor's properties pane: the selected placement (name,
	position, scale, rotation, skew, alpha, remove) when an object of the
	timeline is selected, else the open character (kind, linkage, size, what
	it uses and what uses it, attached tags, its initial text for edit-text
	fields, its texture for external images). Fields commit on Enter or blur
	as pending edits.
-->
<script lang="ts">
	import { CornerDownRight, Eye, EyeOff, Link2, RotateCcw, Trash2 } from "@lucide/svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { compose, decompose, type Decomposed, type SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfCharacter, SwfEntry } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
	}
	const { doc }: Props = $props();

	const entry = $derived(doc.selectedEntry);
	const shown = $derived(entry ? (doc.chars.get(entry.character) ?? null) : doc.character);
	let lockScale = $state(true);
	let confirmRemove = $state(false);

	const fmt = (v: number, d = 2) =>
		Math.abs(v) < 1e-9 ? "0" : v.toFixed(d).replace(/\.?0+$/, "");

	function setPart(e: SwfEntry, key: keyof Decomposed, raw: string) {
		const v = Number(raw);
		if (!Number.isFinite(v)) return;
		const d = decompose(e.matrix);
		const next = { ...d };
		if (key === "sx" || key === "sy") {
			const s = v / 100;
			if (lockScale && d.sx !== 0) {
				const ratio = key === "sx" ? s / d.sx : s / d.sy;
				next.sx = d.sx * ratio;
				next.sy = d.sy * ratio;
			} else next[key] = s;
		} else next[key] = v;
		doc.setMatrix(e, compose(next));
	}

	function setAlpha(e: SwfEntry, percent: number) {
		const base = e.cxform ?? { mul: [1, 1, 1, 1], add: [0, 0, 0, 0] };
		const mul = [...base.mul];
		mul[3] = Math.max(0, Math.min(1, percent / 100));
		const identity = mul.every((m) => m === 1) && base.add.every((a) => a === 0);
		doc.float(e);
		doc.editPlace(doc.sprite!, e.changedBy, {
			cxform: identity ? null : { mul, add: base.add },
		});
	}

	function setName(e: SwfEntry, name: string) {
		doc.editPlace(doc.sprite!, e.placedBy, { name: name.trim() === "" ? null : name.trim() });
	}

	function remove(e: SwfEntry) {
		if (!confirmRemove) {
			confirmRemove = true;
			return;
		}
		confirmRemove = false;
		doc.removeEntry(e);
	}

	function textOf(c: SwfCharacter) {
		return doc.info?.texts.find((t) => t.id === c.id) ?? null;
	}

	function imageOf(c: SwfCharacter) {
		return doc.info?.images.find((i) => (i.id & 0xffff) === c.id) ?? null;
	}

	/** The Texture2D an external image names (`Foo.tga` → `Foo`). */
	function textureFor(fileName: string): number | null {
		const stem = fileName.replace(/\.[a-z0-9]+$/i, "").toLowerCase();
		const row = studio.pkg?.exports.find(
			(e) => e.class.toLowerCase() === "texture2d" && e.name.toLowerCase() === stem,
		);
		return row ? row.i : null;
	}

	const commit = (fn: (v: string) => void) => (event: Event) =>
		fn((event.currentTarget as HTMLInputElement).value);
	const enter = (event: KeyboardEvent) => {
		if (event.key === "Enter") (event.currentTarget as HTMLInputElement).blur();
	};
</script>

{#snippet field(
	label: string,
	value: string,
	onCommit: (v: string) => void,
	suffix = "",
	disabled = false,
)}
	<label class="flex items-center gap-1">
		<span class="w-16 shrink-0 whitespace-nowrap opacity-60">{label}</span>
		<input
			class="input input-xs min-w-0 flex-1 font-mono"
			{value}
			{disabled}
			onchange={commit(onCommit)}
			onkeydown={enter}
		/>
		{#if suffix}<span class="w-3 opacity-50">{suffix}</span>{/if}
	</label>
{/snippet}

{#snippet chips(ids: number[])}
	<div class="flex flex-wrap gap-1">
		{#each ids as id (id)}
			<button
				class="badge badge-ghost badge-sm hover:badge-primary font-mono"
				onclick={() => doc.openCharacter(id)}>{doc.label(id)}</button
			>
		{:else}
			<span class="opacity-50">none</span>
		{/each}
	</div>
{/snippet}

<div class="flex h-full min-h-0 flex-col gap-3 overflow-auto p-2 text-xs">
	{#if entry}
		{@const d = decompose(entry.matrix)}
		{@const alpha = (entry.cxform?.mul[3] ?? 1) * 100}
		<section class="flex flex-col gap-1.5">
			<div class="flex items-center gap-1">
				<span class="font-semibold">Placement</span>
				<span class="font-mono opacity-50">depth {entry.depth}</span>
				<div class="flex-1"></div>
				{#if doc.entryEdited(entry)}
					<button
						class="btn btn-ghost btn-xs"
						title="Drop this object's pending edits"
						onclick={() => doc.revertEntry(entry)}><RotateCcw size={11} /></button
					>
				{/if}
			</div>
			{#if doc.chars.get(entry.character)?.kind === "sprite"}
				<button
					class="btn btn-primary btn-xs justify-start"
					title="Open this clip's own timeline to move what is inside it (double-click on the stage, or Enter)"
					onclick={() => doc.enter(entry)}
					><CornerDownRight size={11} /> Edit inside {entry.name ??
						doc.label(entry.character)}</button
				>
			{/if}
			{@render field(
				"name",
				entry.name ?? "",
				(v) => setName(entry, v),
				"",
				entry.placeCode === 4,
			)}
			{@render field("x", fmt(d.x), (v) => setPart(entry, "x", v), "px")}
			{@render field("y", fmt(d.y), (v) => setPart(entry, "y", v), "px")}
			{@render field("scale x", fmt(d.sx * 100), (v) => setPart(entry, "sx", v), "%")}
			{@render field("scale y", fmt(d.sy * 100), (v) => setPart(entry, "sy", v), "%")}
			<label class="flex items-center gap-1 pl-16 opacity-80">
				<input type="checkbox" class="checkbox checkbox-xs" bind:checked={lockScale} /> keep proportions
			</label>
			{@render field("rotation", fmt(d.rot), (v) => setPart(entry, "rot", v), "°")}
			{@render field("skew", fmt(d.skew), (v) => setPart(entry, "skew", v), "°")}
			{@render field("alpha", fmt(alpha, 1), (v) => setAlpha(entry, Number(v)), "%")}
			<div class="flex flex-wrap gap-1 pt-1">
				<button
					class="btn btn-ghost btn-xs"
					title={alpha === 0 ? "Make it visible again (alpha 100%)" : "Hide it (alpha 0)"}
					onclick={() => setAlpha(entry, alpha === 0 ? 100 : 0)}
				>
					{#if alpha === 0}<Eye size={11} /> Show{:else}<EyeOff size={11} /> Hide{/if}
				</button>
				<button
					class="btn btn-xs {confirmRemove ? 'btn-error' : 'btn-ghost'}"
					onclick={() => remove(entry)}
					onblur={() => (confirmRemove = false)}
					title="Delete the tag that places it and every later change of it"
				>
					<Trash2 size={11} />
					{confirmRemove ? "Click again to remove" : "Remove"}
				</button>
			</div>
			<p class="font-mono text-[10px] opacity-50">
				{entry.placeCode === 70
					? "PlaceObject3"
					: entry.placeCode === 26
						? "PlaceObject2"
						: "PlaceObject"}
				#{entry.placedBy}{entry.matrixBy !== entry.placedBy
					? `, moved by #${entry.matrixBy}`
					: ""}
				· since frame {entry.since + 1}{entry.ratio !== null
					? ` · ratio ${entry.ratio}`
					: ""}{entry.clipDepth !== null
					? ` · masks to depth ${entry.clipDepth}`
					: ""}{entry.extras ? " · filters/blend kept" : ""}{entry.className
					? ` · class ${entry.className}`
					: ""}
			</p>
		</section>
	{/if}

	{#if shown}
		{@const c = shown}
		<section class="flex flex-col gap-1.5">
			<div class="flex items-center gap-1">
				<span class="font-semibold capitalize"
					>{c.kind === "edittext" ? "Edit text" : c.kind}</span
				>
				<span class="font-mono opacity-50">id {c.id}</span>
				{#if entry}
					<div class="flex-1"></div>
					<button class="btn btn-ghost btn-xs" onclick={() => doc.openCharacter(c.id)}
						>Open</button
					>
				{/if}
			</div>
			<div class="grid grid-cols-[4rem_1fr] gap-x-2 gap-y-1">
				{#if c.names.length > 0}
					<span class="opacity-60">linkage</span>
					<span class="font-mono break-all">{c.names.join(", ")}</span>
				{/if}
				<span class="opacity-60">tag</span>
				<span class="font-mono">{c.tag !== null ? `#${c.tag} ` : ""}{c.tagName}</span>
				{#if c.rect}
					<span class="opacity-60">size</span>
					<span class="font-mono"
						>{fmt(c.rect.w, 1)} × {fmt(c.rect.h, 1)} at {fmt(c.rect.x, 1)}, {fmt(
							c.rect.y,
							1,
						)}</span
					>
				{/if}
				{#if c.kind === "sprite"}
					<span class="opacity-60">frames</span>
					<span class="font-mono">{c.frames}</span>
				{/if}
				{#if c.import}
					<span class="opacity-60">from</span>
					<span class="font-mono break-all">{c.import.url}</span>
				{/if}
			</div>
			{#if c.kind === "edittext"}
				{@const t = textOf(c)}
				{#if t}
					<div class="flex flex-col gap-1">
						<span class="opacity-60"
							>initial text · {t.variable || "no variable"}{t.fontClass
								? ` · ${t.fontClass}`
								: ""}{t.fontHeight ? ` ${t.fontHeight}px` : ""}{t.html
								? " · html"
								: ""}</span
						>
						<textarea
							class="textarea textarea-xs font-mono {doc.isEdited(`text|${t.tag}|0`)
								? 'textarea-warning'
								: ''}"
							rows="3"
							value={doc.value("text", t.tag, t.text ?? "")}
							placeholder={t.text === null ? "(no initial text)" : ""}
							onchange={(e) => doc.editText("text", t.tag, e.currentTarget.value)}
						></textarea>
					</div>
				{/if}
			{/if}
			{#if c.kind === "image"}
				{@const im = imageOf(c)}
				{#if im}
					{@const tex = textureFor(im.fileName)}
					<div class="flex items-center gap-1">
						<span class="w-16 shrink-0 whitespace-nowrap opacity-60">file</span>
						<input
							class="input input-xs min-w-0 flex-1 font-mono {doc.isEdited(
								`image|${im.tag}|0`,
							)
								? 'input-warning'
								: ''}"
							value={doc.value("image", im.tag, im.fileName)}
							onchange={(e) => doc.editText("image", im.tag, e.currentTarget.value)}
						/>
						<button
							class="btn btn-ghost btn-xs"
							disabled={tex === null}
							title={tex === null
								? "no Texture2D of that name in this package"
								: "Go to the texture (replace it there)"}
							onclick={() => tex !== null && studio.select(tex)}
							><Link2 size={11} /></button
						>
					</div>
				{/if}
			{/if}
			<div>
				<div class="mb-0.5 opacity-60">uses</div>
				{@render chips(c.deps)}
			</div>
			<div>
				<div class="mb-0.5 opacity-60">used by</div>
				{@render chips(c.usedBy)}
			</div>
			{#if c.attached.length > 0}
				<div>
					<div class="mb-0.5 opacity-60">attached tags</div>
					<div class="flex flex-wrap gap-1">
						{#each c.attached as a (a.tag)}
							<button
								class="badge badge-ghost badge-sm hover:badge-primary font-mono"
								title="Show in the tag list"
								onclick={() =>
									doc.open(a.name === "DoInitAction" ? "scripts" : "tags")}
								>#{a.tag} {a.name}</button
							>
						{/each}
					</div>
				</div>
			{/if}
		</section>
	{:else if !entry}
		<p class="opacity-60">Select a character in the tree or an object on the stage.</p>
	{/if}
</div>
