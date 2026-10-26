<script lang="ts">
	import WwiseMediaList from "$lib/components/WwiseMediaList.svelte";
	import WwiseOutput from "$lib/components/WwiseOutput.svelte";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import type { WwiseObject } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let info = $state<WwiseObject | null>(null);
	let error = $state<string | null>(null);
	let outDir = $state<string | null>(null);
	let version = $state(0);

	$effect(() => {
		void version;
		void (async () => {
			try {
				info = await rpc("wwise.forObject", { path, index });
			} catch (error_) {
				error = error_ instanceof Error ? error_.message : String(error_);
			}
		})();
	});
</script>

<div class="space-y-2 p-3 text-sm">
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !info}
		<span class="loading loading-sm loading-spinner"></span>
	{:else if !info.pck}
		<p>
			Bank <code>{info.bank}</code> has no <code>{info.bank}.pck</code> in the workspace folder.
		</p>
	{:else}
		<p class="text-xs opacity-70">
			{#if info.eventId !== null}
				Event <code>{info.name}</code> (id {info.eventId}) in
				<code>{basename(info.pck)}</code>
				{#if !info.found}— not found in that bank{:else}plays {info.media.length} media file{info
						.media.length === 1
						? ""
						: "s"}{#if info.trail.length}{" "}
						through {info.trail
							.filter((t) => t.kind !== "sound" && t.kind !== "music track")
							.map((t) => t.kind)
							.join(", ") || "its sound"}{/if}{/if}
			{:else}
				Bank <code>{info.bank}</code> ({basename(info.pck)}): {info.media.length} media files
			{/if}
		</p>
		<WwiseOutput bind:outDir />
		{#key version}
			<WwiseMediaList
				pck={info.pck}
				media={info.media}
				{outDir}
				onchanged={() => version++}
			/>
		{/key}
	{/if}
</div>
