<!--
	What the properties pane edits and exports of one character, by kind:
	a shape's colours and line widths (and Replace with SVG), a static text's
	records, every property of an edit-text field, a font's characters (add
	more from a TrueType/OpenType file, export as TrueType), a sound or binary
	data as a file, a sprite's frames as PNGs. Edits are pending like the
	stage's; the details come from `swf.character` with them applied.
-->
<script lang="ts">
	import { Download, FilePlus2, Shapes } from "@lucide/svelte";
	import { invoke, pickFile } from "$lib/platform/api";
	import { rpc } from "$lib/studio/rpc";
	import { exportFile, exportFrames, exportPng, exportSvg } from "$lib/studio/swf-export";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";
	import type {
		SwfCharacter,
		SwfCharacterDetail,
		SwfEditTextProps,
	} from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		character: SwfCharacter;
	}
	const { doc, character }: Props = $props();

	let detail = $state<SwfCharacterDetail | null>(null);
	let failed = $state<string | null>(null);
	let fit = $state(true);
	let glyphChars = $state("");
	let glyphFile = $state("");
	let pngScale = $state(1);

	const tag = $derived(character.tag);
	const editable = $derived(
		["shape", "text", "edittext", "font", "sound", "binary"].includes(character.kind),
	);

	$effect(() => {
		const id = character.id;
		void doc.version;
		void doc.stamp;
		if (!editable) {
			detail = null;
			return;
		}
		let live = true;
		failed = null;
		rpc("swf.character", { path: doc.path, index: doc.index, id, edits: doc.pending })
			.then((d) => {
				if (live) detail = d;
			})
			.catch((error: unknown) => {
				if (live) failed = error instanceof Error ? error.message : String(error);
			});
		return () => {
			live = false;
		};
	});

	/** `#rrggbbaa` → colour input value and alpha percent. */
	const rgb = (c: string) => c.slice(0, 7);
	const alphaOf = (c: string) =>
		Math.round((Number.parseInt(c.slice(7, 9) || "ff", 16) / 255) * 100);
	const withAlpha = (hex: string, percent: number) =>
		`${hex}${Math.round((Math.max(0, Math.min(100, percent)) / 100) * 255)
			.toString(16)
			.padStart(2, "0")}`;

	function setColor(key: string, current: string, hex?: string, percent?: number) {
		if (tag === null) return;
		doc.setShapeStyle(tag, key, withAlpha(hex ?? rgb(current), percent ?? alphaOf(current)));
	}

	async function replaceWithSvg() {
		if (tag === null) return;
		const file = await pickFile("Replace the shape with an SVG", [
			{ name: "SVG", extensions: ["svg"] },
		]);
		if (!file) return;
		try {
			const svg = await invoke("fs:read-text", { path: file });
			doc.replaceShape(tag, svg, fit);
		} catch (error) {
			toasts.error(error, "Could not read the SVG");
		}
	}

	async function pickGlyphFile() {
		const f = await pickFile("Font with the characters", [
			{ name: "Fonts", extensions: ["ttf", "otf", "ttc"] },
		]);
		if (f) glyphFile = f;
	}

	function addGlyphs() {
		if (tag === null || !glyphFile || !glyphChars) return;
		const pending = doc.editOf<"fontGlyphs">(`fontGlyphs|${tag}`);
		// Several additions from the same file merge into one edit.
		const chars =
			pending && pending.file === glyphFile ? pending.chars + glyphChars : glyphChars;
		doc.addGlyphs(tag, glyphFile, chars);
		glyphChars = "";
	}

	const PRESETS: [string, string][] = [
		[
			"Latin-1",
			" !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖ×ØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõö÷øùúûüýþÿ",
		],
		[
			"Latin Extended-A",
			Array.from({ length: 128 }, (_, i) => String.fromCharCode(0x100 + i)).join(""),
		],
		["Cyrillic", Array.from({ length: 96 }, (_, i) => String.fromCharCode(0x400 + i)).join("")],
		["Greek", Array.from({ length: 63 }, (_, i) => String.fromCharCode(0x391 + i)).join("")],
	];

	function setText(props: Partial<SwfEditTextProps>) {
		if (tag !== null) doc.setEditText(tag, props);
	}
	const num = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
	const FLAGS: [keyof SwfEditTextProps, string][] = [
		["multiline", "multiline"],
		["wordWrap", "word wrap"],
		["html", "HTML"],
		["readOnly", "read-only"],
		["noSelect", "not selectable"],
		["password", "password"],
		["autoSize", "auto size"],
		["border", "border"],
		["useOutlines", "embedded glyphs"],
		["wasStatic", "was static"],
	];
</script>

{#snippet exportRow()}
	<div class="flex flex-wrap items-center gap-1">
		<span class="w-16 shrink-0 opacity-60">export</span>
		<button
			class="btn btn-ghost btn-xs"
			onclick={() =>
				exportSvg(
					doc,
					character.kind === "sprite"
						? { kind: "frame", id: character.id, frame: 0 }
						: { kind: "character", id: character.id },
				)}><Download size={11} /> SVG</button
		>
		<button
			class="btn btn-ghost btn-xs"
			onclick={() =>
				exportPng(
					doc,
					character.kind === "sprite"
						? { kind: "frame", id: character.id, frame: 0 }
						: { kind: "character", id: character.id },
					pngScale,
				)}><Download size={11} /> PNG</button
		>
		<select class="select select-xs w-14" bind:value={pngScale} title="PNG scale">
			<option value={1}>1×</option>
			<option value={2}>2×</option>
			<option value={4}>4×</option>
		</select>
	</div>
{/snippet}

{#if character.kind === "sprite"}
	<section class="flex flex-col gap-1.5">
		{@render exportRow()}
		{#if character.frames > 1}
			<button
				class="btn btn-ghost btn-xs justify-start"
				title="Every frame's layout as numbered PNGs, all with the same area"
				onclick={() => exportFrames(doc, character.id, pngScale)}
				><Download size={11} /> All {character.frames} frames as PNGs…</button
			>
		{/if}
	</section>
{:else if editable}
	<section class="flex flex-col gap-1.5">
		{#if failed}
			<p class="text-error">{failed}</p>
		{:else if !detail}
			<span class="loading loading-xs loading-spinner opacity-50"></span>
		{:else if detail.kind === "shape"}
			{#if doc.isEdited(`shapeSvg|${tag}`)}
				<p class="text-warning">Redrawn from an SVG (pending).</p>
			{/if}
			{#if detail.colors.length > 0}
				<div class="opacity-60">colours</div>
				{#each detail.colors as c (c.key)}
					<div class="flex items-center gap-1">
						<span class="w-16 shrink-0 truncate font-mono opacity-50" title={c.key}
							>{c.key.startsWith("stop:")
								? `stop ${c.key.split(":")[3]}`
								: `fill ${Number(c.key.split(":")[2]) + 1}`}</span
						>
						<input
							type="color"
							class="h-5 w-8 cursor-pointer rounded border-0 bg-transparent p-0 {doc.isEdited(
								`shapeColor|${tag}|${c.key}`,
							)
								? 'outline-warning outline'
								: ''}"
							value={rgb(c.color)}
							onchange={(e) => setColor(c.key, c.color, e.currentTarget.value)}
						/>
						<input
							class="input input-xs w-20 font-mono"
							value={rgb(c.color)}
							onchange={(e) =>
								setColor(
									c.key,
									c.color,
									e.currentTarget.value.startsWith("#")
										? e.currentTarget.value
										: `#${e.currentTarget.value}`,
								)}
						/>
						{#if detail.alpha}
							<input
								type="number"
								min="0"
								max="100"
								class="input input-xs w-14 font-mono"
								value={alphaOf(c.color)}
								title="alpha %"
								onchange={(e) => setColor(c.key, c.color, undefined, num(e))}
							/><span class="opacity-50">%</span>
						{/if}
					</div>
				{/each}
			{/if}
			{#if detail.lines.length > 0}
				<div class="opacity-60">lines</div>
				{#each detail.lines as l (l.key)}
					<div class="flex items-center gap-1">
						<input
							type="number"
							min="0"
							step="0.05"
							class="input input-xs w-16 font-mono {doc.isEdited(
								`shapeColor|${tag}|${l.key}`,
							)
								? 'input-warning'
								: ''}"
							value={l.width}
							title="width (px; 0 is a hairline)"
							onchange={(e) =>
								tag !== null && doc.setShapeStyle(tag, l.key, String(num(e)))}
						/><span class="opacity-50">px</span>
						<input
							type="color"
							class="h-5 w-8 cursor-pointer rounded border-0 bg-transparent p-0"
							value={rgb(l.color)}
							onchange={(e) => setColor(l.colorKey, l.color, e.currentTarget.value)}
						/>
						{#if detail.alpha}
							<input
								type="number"
								min="0"
								max="100"
								class="input input-xs w-14 font-mono"
								value={alphaOf(l.color)}
								title="alpha %"
								onchange={(e) => setColor(l.colorKey, l.color, undefined, num(e))}
							/><span class="opacity-50">%</span>
						{/if}
					</div>
				{/each}
			{/if}
			{#if detail.colors.length === 0 && detail.lines.length === 0}
				<p class="opacity-60">
					{detail.bitmapFills > 0
						? "Draws an image: replace the picture through the image."
						: "No colours."}
				</p>
			{/if}
			{#if !detail.alpha}
				<p class="opacity-50">{detail.tagName}: colours have no alpha.</p>
			{/if}
			<div class="flex flex-wrap items-center gap-1">
				<button
					class="btn btn-xs"
					title="Redraw the shape from an SVG (paths, gradients, strokes; texts and images inside it are left out)"
					onclick={replaceWithSvg}><Shapes size={11} /> Replace with SVG…</button
				>
				<label
					class="flex items-center gap-1"
					title="Scale the SVG into the shape's current bounds (else one SVG unit per pixel from the shape's origin)"
				>
					<input type="checkbox" class="checkbox checkbox-xs" bind:checked={fit} /> fit to bounds
				</label>
			</div>
			{@render exportRow()}
		{:else if detail.kind === "text"}
			{#each detail.records as r, i (i)}
				<div class="flex flex-col gap-1">
					<span class="opacity-60"
						>record {i + 1} · {r.fontName ?? `font ${r.font}`} {r.height}px</span
					>
					<div class="flex items-center gap-1">
						<input
							class="input input-xs min-w-0 flex-1 font-mono {doc.isEdited(
								`staticText|${tag}|${i}`,
							)
								? 'input-warning'
								: ''}"
							value={r.text}
							onchange={(e) =>
								tag !== null &&
								doc.setStaticText(tag, i, { text: e.currentTarget.value })}
						/>
						<input
							type="color"
							class="h-5 w-8 cursor-pointer rounded border-0 bg-transparent p-0"
							value={rgb(r.color)}
							onchange={(e) =>
								tag !== null &&
								doc.setStaticText(tag, i, {
									color: withAlpha(e.currentTarget.value, alphaOf(r.color)),
								})}
						/>
					</div>
				</div>
			{/each}
			<p class="opacity-50">
				Characters come from the record's font: add missing ones to the font first.
			</p>
			{@render exportRow()}
		{:else if detail.kind === "edittext"}
			{@const p = detail.props}
			<div
				class="grid grid-cols-[4rem_1fr] items-center gap-x-2 gap-y-1 {doc.isEdited(
					`editText|${tag}`,
				)
					? 'border-warning border-l-2 pl-1'
					: ''}"
			>
				<span class="opacity-60">font</span>
				<select
					class="select select-xs w-full"
					value={p.font !== null
						? `id:${p.font}`
						: p.fontClass !== null
							? `class:${p.fontClass}`
							: ""}
					onchange={(e) => {
						const v = e.currentTarget.value;
						if (v.startsWith("id:")) setText({ font: Number(v.slice(3)) });
						else if (v.startsWith("class:")) setText({ fontClass: v.slice(6) });
					}}
				>
					{#if p.font === null && p.fontClass === null}<option value="">(none)</option
						>{/if}
					{#if p.fontClass !== null}<option value={`class:${p.fontClass}`}
							>{p.fontClass} (class)</option
						>{/if}
					{#each detail.fonts as f (f.id)}
						<option value={`id:${f.id}`}
							>{f.name || `font ${f.id}`}{f.imported ? " (imported)" : ""} · {f.id}</option
						>
					{/each}
				</select>
				<span class="opacity-60">size</span>
				<label class="flex items-center gap-1"
					><input
						type="number"
						step="0.5"
						min="0"
						class="input input-xs w-20 font-mono"
						value={p.height ?? ""}
						onchange={(e) => setText({ height: num(e) })}
					/><span class="opacity-50">px</span></label
				>
				<span class="opacity-60">colour</span>
				<div class="flex items-center gap-1">
					<input
						type="color"
						class="h-5 w-8 cursor-pointer rounded border-0 bg-transparent p-0"
						value={rgb(p.color ?? "#000000ff")}
						onchange={(e) =>
							setText({
								color: withAlpha(
									e.currentTarget.value,
									alphaOf(p.color ?? "#000000ff"),
								),
							})}
					/>
					<input
						type="number"
						min="0"
						max="100"
						class="input input-xs w-14 font-mono"
						value={alphaOf(p.color ?? "#000000ff")}
						title="alpha %"
						onchange={(e) =>
							setText({ color: withAlpha(rgb(p.color ?? "#000000ff"), num(e)) })}
					/><span class="opacity-50">%</span>
				</div>
				<span class="opacity-60">align</span>
				<select
					class="select select-xs w-full"
					value={p.align}
					onchange={(e) =>
						setText({ align: e.currentTarget.value as SwfEditTextProps["align"] })}
				>
					{#each ["left", "center", "right", "justify"] as a (a)}<option value={a}
							>{a}</option
						>{/each}
				</select>
				<span class="opacity-60">margins</span>
				<div class="flex items-center gap-1">
					<input
						type="number"
						step="0.5"
						class="input input-xs w-14 font-mono"
						value={p.leftMargin}
						title="left (px)"
						onchange={(e) => setText({ leftMargin: num(e) })}
					/>
					<input
						type="number"
						step="0.5"
						class="input input-xs w-14 font-mono"
						value={p.rightMargin}
						title="right (px)"
						onchange={(e) => setText({ rightMargin: num(e) })}
					/>
				</div>
				<span class="opacity-60">indent</span>
				<input
					type="number"
					step="0.5"
					class="input input-xs w-20 font-mono"
					value={p.indent}
					onchange={(e) => setText({ indent: num(e) })}
				/>
				<span class="opacity-60">leading</span>
				<input
					type="number"
					step="0.5"
					class="input input-xs w-20 font-mono"
					value={p.leading}
					onchange={(e) => setText({ leading: num(e) })}
				/>
				<span class="opacity-60">max length</span>
				<input
					type="number"
					min="0"
					class="input input-xs w-20 font-mono"
					value={p.maxLength ?? ""}
					placeholder="none"
					onchange={(e) =>
						setText({ maxLength: e.currentTarget.value === "" ? null : num(e) })}
				/>
				<span class="opacity-60">variable</span>
				<input
					class="input input-xs w-full font-mono"
					value={p.variable}
					onchange={(e) => setText({ variable: e.currentTarget.value })}
				/>
				<span class="opacity-60">box</span>
				<div class="flex items-center gap-1">
					{#each [["x", p.bounds.x], ["y", p.bounds.y], ["w", p.bounds.w], ["h", p.bounds.h]] as [k, v] (k)}
						<input
							type="number"
							step="0.05"
							class="input input-xs w-14 font-mono"
							value={v}
							title={String(k)}
							onchange={(e) =>
								setText({ bounds: { ...p.bounds, [k as string]: num(e) } })}
						/>
					{/each}
				</div>
			</div>
			<div class="flex flex-wrap gap-x-3 gap-y-1">
				{#each FLAGS as [k, label] (k)}
					<label class="flex items-center gap-1">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							checked={p[k] as boolean}
							onchange={(e) => setText({ [k]: e.currentTarget.checked })}
						/>
						{label}
					</label>
				{/each}
			</div>
		{:else if detail.kind === "font"}
			<div class="grid grid-cols-[4rem_1fr] gap-x-2 gap-y-1">
				<span class="opacity-60">name</span>
				<span class="font-mono"
					>{detail.name}{detail.bold ? " bold" : ""}{detail.italic ? " italic" : ""}</span
				>
				<span class="opacity-60">glyphs</span>
				<span class="font-mono"
					>{detail.glyphs}{detail.layout ? "" : " · no layout"}{detail.alignZones
						? " · align zones"
						: ""}</span
				>
			</div>
			<div
				class="bg-base-200 max-h-24 overflow-auto rounded p-1 font-mono text-[11px] break-all"
			>
				{detail.chars || "(device font: no glyphs)"}
			</div>
			{#if detail.editable}
				{@const pending = doc.editOf<"fontGlyphs">(`fontGlyphs|${tag}`)}
				<div class="opacity-60">add characters</div>
				{#if pending}
					<p class="text-warning">
						Pending: “{pending.chars.slice(0, 60)}{pending.chars.length > 60
							? "…"
							: ""}” from {pending.file.replace(/^.*[\\/]/, "")}
					</p>
				{/if}
				<div class="flex gap-1">
					<input
						class="input input-xs min-w-0 flex-1 font-mono"
						placeholder="from file…"
						readonly
						value={glyphFile.replace(/^.*[\\/]/, "")}
						title={glyphFile}
					/>
					<button class="btn btn-xs" onclick={pickGlyphFile}>Font file…</button>
				</div>
				<textarea
					class="textarea textarea-xs font-mono"
					rows="2"
					bind:value={glyphChars}
					placeholder="characters to add (those already there are skipped)"></textarea>
				<div class="flex flex-wrap gap-1">
					{#each PRESETS as [name, chars] (name)}
						<button class="btn btn-ghost btn-xs" onclick={() => (glyphChars += chars)}
							>+ {name}</button
						>
					{/each}
				</div>
				<button class="btn btn-xs" disabled={!glyphFile || !glyphChars} onclick={addGlyphs}
					><FilePlus2 size={11} /> Add to the font</button
				>
				<p class="opacity-50">
					Glyphs are inserted in character order; static texts using the font follow. The
					file's outlines and advances are scaled to the font.
				</p>
			{/if}
			<div class="flex items-center gap-1">
				<span class="w-16 shrink-0 opacity-60">export</span>
				<button
					class="btn btn-ghost btn-xs"
					disabled={detail.glyphs === 0}
					onclick={() => exportFile(doc, character.id, "ttf", "ttf")}
					><Download size={11} /> TrueType</button
				>
			</div>
		{:else if detail.kind === "sound"}
			<p class="font-mono">
				{detail.format} · {detail.rate} Hz · {detail.channels === 2
					? "stereo"
					: "mono"}{detail.samples ? ` · ${detail.samples} samples` : ""}
			</p>
			<button
				class="btn btn-ghost btn-xs justify-start"
				disabled={!detail.ext}
				onclick={() =>
					detail?.kind === "sound" &&
					detail.ext &&
					exportFile(doc, character.id, "sound", detail.ext)}
				><Download size={11} /> Export {detail.ext?.toUpperCase() ??
					"(not supported)"}</button
			>
		{:else if detail.kind === "binary"}
			<p class="font-mono">{detail.bytes} bytes</p>
			<button
				class="btn btn-ghost btn-xs justify-start"
				onclick={() => exportFile(doc, character.id, "binary", "bin")}
				><Download size={11} /> Export</button
			>
		{/if}
	</section>
{:else if ["morph", "button", "image", "subimage", "bitmap"].includes(character.kind)}
	<section class="flex flex-col gap-1.5">
		{@render exportRow()}
	</section>
{/if}
