<script lang="ts">
	import { Check, ExternalLink, RotateCcw } from "@lucide/svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { SwfEdit, SwfInfo } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let info = $state<SwfInfo | null>(null);
	let error = $state<string | null>(null);
	let tab = $state<"texts" | "scripts" | "images" | "linkage" | "tags">("texts");
	/** Pending edits keyed `${kind}|${tag}|${index}`. */
	let edits = $state<Record<string, SwfEdit>>({});
	let listing = $state<{ tag: number; text: string } | null>(null);
	let saving = $state(false);
	let tagFilter = $state("");

	const pending = $derived(Object.values(edits));

	async function load() {
		info = null;
		error = null;
		edits = {};
		listing = null;
		try {
			info = await rpc("swf.info", { path, index });
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		}
	}

	$effect(() => {
		void path;
		void index;
		void load();
	});

	function edit(kind: SwfEdit["kind"], tag: number, text: string, idx?: number) {
		const key = `${kind}|${tag}|${idx ?? 0}`;
		const next = { ...edits };
		next[key] = { kind, tag, index: idx, text };
		edits = next;
	}

	function value(kind: SwfEdit["kind"], tag: number, original: string, idx?: number): string {
		return edits[`${kind}|${tag}|${idx ?? 0}`]?.text ?? original;
	}

	async function apply() {
		if (!studio.pkg || pending.length === 0) return;
		saving = true;
		try {
			const r = await rpc("swf.apply", { path, index, edits: pending });
			studio.markDirty(path);
			studio.undoLabels = r.undo;
			toasts.push(
				"success",
				`${r.edits} edits, ${r.bytes} bytes; save the package to keep them`,
				"Movie updated",
			);
			await load();
		} catch (error_) {
			toasts.error(error_, "Could not apply the edits");
		} finally {
			saving = false;
		}
	}

	async function showScript(tag: number) {
		try {
			const r = await rpc("swf.script", { path, index, tag });
			listing = { tag, text: r.listing };
		} catch (error_) {
			toasts.error(error_, "Could not disassemble");
		}
	}

	/** The texture export an external image refers to (`Foo.tga` → `Foo`). */
	function textureFor(fileName: string): number | null {
		const stem = fileName.replace(/\.[a-z0-9]+$/i, "").toLowerCase();
		const row = studio.pkg?.exports.find(
			(e) => e.class.toLowerCase() === "texture2d" && e.name.toLowerCase() === stem,
		);
		return row ? row.i : null;
	}

	const visibleTags = $derived(
		info
			? info.tags.filter((t) => {
					const q = tagFilter.trim().toLowerCase();
					return (
						q === "" ||
						t.name.toLowerCase().includes(q) ||
						t.detail.toLowerCase().includes(q)
					);
				})
			: [],
	);
</script>

{#if error}
	<div class="alert alert-error text-xs">{error}</div>
{:else if !info}
	<div class="flex items-center gap-2 p-3 text-xs opacity-60">
		<span class="loading loading-xs loading-spinner"></span> Reading the movie…
	</div>
{:else}
	<div class="flex flex-wrap items-center gap-2 px-1 text-xs">
		<span class="font-mono">{info.signature} v{info.version}</span>
		<span class="opacity-60"
			>{info.width}×{info.height} · {info.frameRate} fps · {info.frames} frames · {info.tags
				.length} tags{info.compressed ? " · compressed" : ""}</span
		>
		<div class="flex-1"></div>
		{#if pending.length > 0}
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => (edits = {})}
				title="Drop the pending edits"><RotateCcw size={11} /></button
			>
			<button class="btn btn-primary btn-xs" onclick={apply} disabled={saving}>
				{#if saving}<span class="loading loading-xs loading-spinner"></span>{:else}<Check
						size={11}
					/>{/if}
				Apply {pending.length} edit{pending.length > 1 ? "s" : ""}
			</button>
		{/if}
	</div>
	<div role="tablist" class="tabs-box tabs tabs-xs mx-1 mt-1 w-fit">
		<button
			role="tab"
			class="tab {tab === 'texts' ? 'tab-active' : ''}"
			onclick={() => (tab = "texts")}>Texts ({info.texts.length})</button
		>
		<button
			role="tab"
			class="tab {tab === 'scripts' ? 'tab-active' : ''}"
			onclick={() => (tab = "scripts")}>Scripts ({info.scripts.length})</button
		>
		<button
			role="tab"
			class="tab {tab === 'images' ? 'tab-active' : ''}"
			onclick={() => (tab = "images")}>Images ({info.images.length})</button
		>
		<button
			role="tab"
			class="tab {tab === 'linkage' ? 'tab-active' : ''}"
			onclick={() => (tab = "linkage")}
			>Linkage ({info.exports.length}/{info.imports.length})</button
		>
		<button
			role="tab"
			class="tab {tab === 'tags' ? 'tab-active' : ''}"
			onclick={() => (tab = "tags")}>Tags</button
		>
	</div>
	<div class="min-h-0 flex-1 overflow-auto p-1 text-xs">
		{#if tab === "texts"}
			{#if info.texts.length === 0}
				<p class="p-2 opacity-60">
					No edit-text fields; the UI's texts come from the language files (Assembly tab)
					or from scripts.
				</p>
			{/if}
			<table class="table-xs table font-mono">
				<thead
					><tr><th>id</th><th>variable</th><th>font</th><th>initial text</th></tr></thead
				>
				<tbody>
					{#each info.texts as t (t.tag)}
						<tr>
							<td class="opacity-60">{t.id}</td>
							<td>{t.variable}</td>
							<td class="opacity-60"
								>{t.fontClass ?? t.fontId ?? ""}{t.fontHeight
									? ` ${t.fontHeight}`
									: ""}{t.html ? " html" : ""}</td
							>
							<td>
								<input
									class="input input-xs w-full {edits[`text|${t.tag}|0`]
										? 'input-warning'
										: ''}"
									value={value("text", t.tag, t.text ?? "")}
									placeholder={t.text === null ? "(no initial text)" : ""}
									onchange={(e) => edit("text", t.tag, e.currentTarget.value)}
								/>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{:else if tab === "scripts"}
			<p class="px-1 pb-1 opacity-60">
				ActionScript 2. The strings of the top-level actions (constant pools, pushes) can be
				edited; the bytecode is shown as a listing.
			</p>
			{#each info.scripts as sc (sc.tag)}
				<details class="border-base-300 mb-1 rounded border">
					<summary class="cursor-pointer px-2 py-1 font-mono">
						{sc.kind}{sc.sprite !== null ? ` sprite ${sc.sprite}` : ""} · {sc.bytes} bytes
						· {sc.strings.length} strings
						{#if sc.strings[0]}<span class="opacity-50">
								· {sc.strings
									.slice(0, 4)
									.map((s) => s.text)
									.join(" ")}</span
							>{/if}
					</summary>
					<div class="px-2 pb-2">
						<button class="btn btn-ghost btn-xs" onclick={() => showScript(sc.tag)}
							>Listing</button
						>
						{#if listing?.tag === sc.tag}
							<pre
								class="bg-base-300 selectable max-h-96 overflow-auto rounded p-2 font-mono text-[10px]">{listing.text}</pre>
						{/if}
						<table class="table-xs table font-mono">
							<tbody>
								{#each sc.strings as s (s.index)}
									<tr>
										<td class="w-8 opacity-50">{s.index}</td>
										<td class="w-20 opacity-50">{s.action}</td>
										<td>
											<input
												class="input input-xs w-full {edits[
													`string|${sc.tag}|${s.index}`
												]
													? 'input-warning'
													: ''}"
												value={value("string", sc.tag, s.text, s.index)}
												onchange={(e) =>
													edit(
														"string",
														sc.tag,
														e.currentTarget.value,
														s.index,
													)}
											/>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</details>
			{/each}
		{:else if tab === "images"}
			<p class="px-1 pb-1 opacity-60">
				External images: the engine loads each file name as a Texture2D of this package.
				Replace the texture to change the picture; rename the reference to point at another
				texture.
			</p>
			<table class="table-xs table font-mono">
				<thead><tr><th>id</th><th>size</th><th>format</th><th>file</th><th></th></tr></thead
				>
				<tbody>
					{#each info.images as im (im.tag)}
						{@const tex = textureFor(im.fileName)}
						<tr>
							<td class="opacity-60">{im.id}</td>
							<td>{im.width}×{im.height}</td>
							<td class="opacity-60">{im.format}</td>
							<td>
								<input
									class="input input-xs w-full {edits[`image|${im.tag}|0`]
										? 'input-warning'
										: ''}"
									value={value("image", im.tag, im.fileName)}
									onchange={(e) => edit("image", im.tag, e.currentTarget.value)}
								/>
							</td>
							<td>
								{#if tex !== null}
									<button
										class="btn btn-ghost btn-xs"
										title="Go to the texture"
										onclick={() => studio.select(tex)}
										><ExternalLink size={11} /></button
									>
								{:else}
									<span
										class="opacity-40"
										title="no Texture2D of that name in this package">?</span
									>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{:else if tab === "linkage"}
			<div class="grid grid-cols-2 gap-2">
				<div>
					<div class="mb-1 font-semibold">Exported symbols</div>
					{#each info.exports as ex (`${ex.tag}-${ex.id}`)}
						<div class="font-mono">
							<span class="opacity-50">{ex.id}</span>
							{ex.name}
						</div>
					{/each}
				</div>
				<div>
					<div class="mb-1 font-semibold">Imports</div>
					{#each info.imports as im (im.tag)}
						<div class="font-mono">{im.url}</div>
						{#each im.symbols as sy (sy.id)}
							<div class="pl-3 font-mono opacity-70">
								<span class="opacity-50">{sy.id}</span>
								{sy.name}
							</div>
						{/each}
					{/each}
				</div>
			</div>
		{:else}
			<input
				class="input input-xs mb-1 w-full font-mono"
				placeholder="Filter tags…"
				bind:value={tagFilter}
			/>
			<table class="table-xs table font-mono">
				<thead
					><tr><th>#</th><th>tag</th><th>id</th><th>bytes</th><th>detail</th></tr></thead
				>
				<tbody>
					{#each visibleTags as t (t.index)}
						<tr>
							<td class="opacity-50">{t.index}</td>
							<td>{t.name}{t.name === "?" ? ` (${t.code})` : ""}</td>
							<td class="opacity-60">{t.id ?? ""}</td>
							<td class="opacity-60">{t.len}</td>
							<td class="max-w-[30rem] truncate" title={t.detail}>{t.detail}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
{/if}
