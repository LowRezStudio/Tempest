<!--
	The movie editor's tag views: edit-text fields (initial text), scripts
	(P-code listing and their top-level strings), external images, linkage
	(exports and imports) and every tag. Edits go to the document's pending
	list like the stage's.
-->
<script lang="ts">
	import { ExternalLink } from "@lucide/svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";

	interface Props {
		doc: SwfDoc;
		section: "texts" | "scripts" | "images" | "linkage" | "tags";
	}
	const { doc, section }: Props = $props();

	const info = $derived(doc.info);
	let listing = $state<{ tag: number; text: string } | null>(null);
	let tagFilter = $state("");

	async function showScript(tag: number) {
		if (listing?.tag === tag) {
			listing = null;
			return;
		}
		try {
			const r = await rpc("swf.script", { path: doc.path, index: doc.index, tag });
			listing = { tag, text: r.listing };
		} catch (error) {
			toasts.error(error, "Could not disassemble");
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

	/** The class a DoInitAction defines, from its sprite's linkage name (`__Packages.a.B`). */
	function scriptName(sprite: number | null): string {
		if (sprite === null) return "";
		const n = doc.chars.get(sprite)?.names[0];
		return n ? n.replace(/^__Packages\./, "") : doc.label(sprite);
	}

	const visibleTags = $derived(
		info
			? info.tags.filter((t) => {
					const q = tagFilter.trim().toLowerCase();
					return (
						q === "" ||
						t.name.toLowerCase().includes(q) ||
						t.detail.toLowerCase().includes(q) ||
						String(t.id ?? "") === q
					);
				})
			: [],
	);
</script>

{#if !info}
	<div class="p-3 text-xs opacity-60">Reading the movie…</div>
{:else}
	<div class="h-full min-h-0 overflow-auto p-1 text-xs">
		{#if section === "texts"}
			{#if info.texts.length === 0}
				<p class="p-2 opacity-60">
					No edit-text fields; the UI's texts come from the language files (Assembly tab)
					or from scripts.
				</p>
			{/if}
			<table class="table-xs table-pin-rows table font-mono">
				<thead
					><tr><th>id</th><th>variable</th><th>font</th><th>initial text</th></tr></thead
				>
				<tbody>
					{#each info.texts as t (t.tag)}
						<tr>
							<td
								><button
									class="link opacity-60"
									onclick={() => doc.openCharacter(t.id)}>{t.id}</button
								></td
							>
							<td>{t.variable}</td>
							<td class="opacity-60"
								>{t.fontClass ?? t.fontId ?? ""}{t.fontHeight
									? ` ${t.fontHeight}`
									: ""}{t.html ? " html" : ""}</td
							>
							<td>
								<input
									class="input input-xs w-full {doc.isEdited(`text|${t.tag}|0`)
										? 'input-warning'
										: ''}"
									value={doc.value("text", t.tag, t.text ?? "")}
									placeholder={t.text === null ? "(no initial text)" : ""}
									onchange={(e) =>
										doc.editText("text", t.tag, e.currentTarget.value)}
								/>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{:else if section === "scripts"}
			<p class="px-1 pb-1 opacity-60">
				ActionScript 2. The strings of the top-level actions (constant pools, pushes) can be
				edited; the bytecode is shown as a listing.
			</p>
			{#each info.scripts as sc (sc.tag)}
				<details class="border-base-300 mb-1 rounded border">
					<summary class="cursor-pointer px-2 py-1 font-mono">
						{#if sc.sprite !== null}<span class="font-semibold"
								>{scriptName(sc.sprite)}</span
							> ·
						{/if}{sc.kind} #{sc.tag} · {sc.bytes} bytes · {sc.strings.length} strings
					</summary>
					<div class="px-2 pb-2">
						<div class="flex gap-1">
							<button class="btn btn-ghost btn-xs" onclick={() => showScript(sc.tag)}
								>{listing?.tag === sc.tag ? "Hide listing" : "Listing"}</button
							>
							{#if sc.sprite !== null}
								<button
									class="btn btn-ghost btn-xs"
									onclick={() => doc.openCharacter(sc.sprite!)}
									>Sprite {sc.sprite}</button
								>
							{/if}
						</div>
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
												class="input input-xs w-full {doc.isEdited(
													`string|${sc.tag}|${s.index}`,
												)
													? 'input-warning'
													: ''}"
												value={doc.value("string", sc.tag, s.text, s.index)}
												onchange={(e) =>
													doc.editText(
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
		{:else if section === "images"}
			<p class="px-1 pb-1 opacity-60">
				External images: the engine loads each file name as a Texture2D of this package.
				Replace the texture to change the picture; rename the reference to point at another
				texture.
			</p>
			<table class="table-xs table-pin-rows table font-mono">
				<thead><tr><th>id</th><th>size</th><th>format</th><th>file</th><th></th></tr></thead
				>
				<tbody>
					{#each info.images as im (im.tag)}
						{@const tex = textureFor(im.fileName)}
						<tr>
							<td
								><button
									class="link opacity-60"
									onclick={() => doc.openCharacter(im.id & 0xffff)}
									>{im.id & 0xffff}</button
								></td
							>
							<td>{im.width}×{im.height}</td>
							<td class="opacity-60">{im.format}</td>
							<td>
								<input
									class="input input-xs w-full {doc.isEdited(`image|${im.tag}|0`)
										? 'input-warning'
										: ''}"
									value={doc.value("image", im.tag, im.fileName)}
									onchange={(e) =>
										doc.editText("image", im.tag, e.currentTarget.value)}
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
		{:else if section === "linkage"}
			<div class="grid grid-cols-2 gap-2">
				<div>
					<div class="mb-1 font-semibold">Exported symbols</div>
					{#each info.exports as ex (`${ex.tag}-${ex.id}`)}
						<button
							class="block font-mono hover:underline"
							onclick={() => doc.openCharacter(ex.id)}
						>
							<span class="opacity-50">{ex.id}</span>
							{ex.name}
						</button>
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
			<table class="table-xs table-pin-rows table font-mono">
				<thead
					><tr><th>#</th><th>tag</th><th>id</th><th>bytes</th><th>detail</th></tr></thead
				>
				<tbody>
					{#each visibleTags as t (t.index)}
						<tr>
							<td class="opacity-50">{t.index}</td>
							<td>{t.name}{t.name === "?" ? ` (${t.code})` : ""}</td>
							<td class="opacity-60">
								{#if t.id !== null && doc.chars.has(t.id)}
									<button class="link" onclick={() => doc.openCharacter(t.id!)}
										>{t.id}</button
									>
								{:else}{t.id ?? ""}{/if}
							</td>
							<td class="opacity-60">{t.len}</td>
							<td class="max-w-[30rem] truncate" title={t.detail}>{t.detail}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
{/if}
