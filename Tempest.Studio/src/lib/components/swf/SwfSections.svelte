<!--
	The movie editor's tag views: edit-text fields (initial text), scripts
	(`SwfScripts`: decompiled ActionScript and P-code), external images, linkage
	(exports and imports) and every tag. Edits go to the document's pending
	list like the stage's.
-->
<script lang="ts">
	import { ImagePlus } from "@lucide/svelte";
	import SwfImageCard from "$lib/components/swf/SwfImageCard.svelte";
	import SwfScripts from "$lib/components/swf/SwfScripts.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";

	interface Props {
		doc: SwfDoc;
		section: "texts" | "scripts" | "images" | "linkage" | "tags";
	}
	const { doc, section }: Props = $props();

	const info = $derived(doc.info);
	let tagFilter = $state("");

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
{:else if section === "scripts"}
	<SwfScripts {doc} />
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
		{:else if section === "images"}
			<div class="flex items-start gap-2 px-1 pb-1">
				<p class="flex-1 opacity-60">
					External images: the engine loads each file name as a Texture2D of this package.
					Replace the picture (PNG or DDS) to change it; the movie keeps drawing it at the
					size listed here. Rename the reference to point at another texture.
				</p>
				<button class="btn btn-xs" onclick={() => (doc.addingImage = true)}
					><ImagePlus size={11} /> Add image…</button
				>
			</div>
			<table class="table-xs table-pin-rows table font-mono">
				<thead
					><tr><th>id</th><th>size</th><th>format</th><th>file</th><th>picture</th></tr
					></thead
				>
				<tbody>
					{#each info.images as im (im.tag)}
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
							<td><SwfImageCard {doc} image={im} compact /></td>
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
