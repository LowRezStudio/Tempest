<script lang="ts">
	import { onDestroy } from "svelte";
	import { bytes } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import type { TextureInfo } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let info = $state<TextureInfo | null>(null);
	let error = $state<string | null>(null);
	let url = $state<string | null>(null);
	let mip = $state(0);
	let loading = $state(false);
	let background = $state<"checker" | "black" | "white">("checker");
	let zoom = $state<"fit" | "1x" | "2x">("fit");

	async function load(mipIndex: number) {
		loading = true;
		error = null;
		try {
			if (!info) info = await rpc("texture.info", { path, index });
			const r = await rpc("texture.preview", { path, index, mip: mipIndex });
			if (url) URL.revokeObjectURL(url);
			url = `data:image/png;base64,${r.png}`;
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load(mip);
	});

	onDestroy(() => {
		if (url) URL.revokeObjectURL(url);
	});

	const availableMips = $derived(
		(info?.mips ?? []).filter(
			(m) => m.location.kind === "inline" || m.location.kind === "cache",
		),
	);
	const locText = (m: TextureInfo["mips"][number]) => {
		const l = m.location;
		if (l.kind === "cache") return `${l.file}.tfc @${l.offset} (${bytes(l.size)})`;
		if (l.kind === "inline") return `inline (${bytes(m.storedBytes)})`;
		return l.kind === "unused" ? "stripped by the cooker" : "missing from the texture index";
	};
</script>

<div class="flex h-full flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
		{#if info}
			<span class="font-mono">{info.width}×{info.height}</span>
			<span class="badge badge-ghost badge-sm font-mono">{info.format}</span>
			{#if info.lightmap}<span class="badge badge-sm badge-warning">lightmap</span>{/if}
			{#if !info.supported}<span class="badge badge-sm badge-error">format not decodable</span
				>{/if}
			<span class="font-mono opacity-60">{info.path}</span>
			<div class="flex-1"></div>
			<label class="flex items-center gap-1">
				mip
				<select class="select select-xs w-36" bind:value={mip}>
					{#each availableMips as m (m.index)}
						<option value={m.index}>{m.index}: {m.width}×{m.height}</option>
					{/each}
				</select>
			</label>
			<select class="select select-xs w-20" bind:value={zoom}>
				<option value="fit">fit</option>
				<option value="1x">1×</option>
				<option value="2x">2×</option>
			</select>
			<select class="select select-xs w-24" bind:value={background}>
				<option value="checker">checker</option>
				<option value="black">black</option>
				<option value="white">white</option>
			</select>
		{/if}
	</div>
	<div
		class="relative min-h-0 flex-1 overflow-auto {background === 'black'
			? 'bg-black'
			: background === 'white'
				? 'bg-white'
				: 'checker'}"
	>
		{#if loading && !url}
			<div class="flex h-full items-center justify-center">
				<span class="loading loading-spinner"></span>
			</div>
		{:else if error}
			<div class="text-error p-4 text-sm">{error}</div>
		{:else if url}
			<div class="flex min-h-full items-center justify-center p-2">
				<img
					src={url}
					alt="texture"
					class="{zoom === 'fit'
						? 'max-h-full max-w-full object-contain'
						: ''} image-render-pixel"
					style={zoom === "1x"
						? "width:auto;height:auto"
						: zoom === "2x"
							? `width:${(info?.mips[mip]?.width ?? 0) * 2}px`
							: "max-height: calc(100vh - 16rem)"}
				/>
			</div>
		{/if}
		{#if loading && url}
			<span class="loading loading-xs loading-spinner absolute top-2 right-2"></span>
		{/if}
	</div>
	{#if info}
		<div
			class="border-base-300 max-h-28 shrink-0 overflow-auto border-t px-3 py-1 font-mono text-[11px]"
		>
			{#each info.mips as m (m.index)}
				<div
					class={m.location.kind === "unused" || m.location.kind === "missing"
						? "opacity-50"
						: ""}
				>
					mip {m.index}: {m.width}×{m.height} · {locText(m)}
				</div>
			{/each}
		</div>
	{/if}
</div>

<style>
	.checker {
		background-color: #808080;
		background-image:
			linear-gradient(45deg, #666 25%, transparent 25%),
			linear-gradient(-45deg, #666 25%, transparent 25%),
			linear-gradient(45deg, transparent 75%, #666 75%),
			linear-gradient(-45deg, transparent 75%, #666 75%);
		background-size: 16px 16px;
		background-position:
			0 0,
			0 8px,
			8px -8px,
			-8px 0;
	}
	.image-render-pixel {
		image-rendering: auto;
	}
</style>
