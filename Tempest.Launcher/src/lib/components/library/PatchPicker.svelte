<script lang="ts">
	import { ExternalLink } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { WIKI_BASE_URL } from "$lib/rigby/constants";
	import PatchTimeline from "./PatchTimeline.svelte";
	import type { Patch } from "$lib/utils/versions";

	interface Props {
		/** Oldest first, as in versions.json */
		patches: Patch[];
		/** The picked patch's id */
		value: string;
		/** A patch's id whose tick stands out in the accent color */
		highlight?: string;
	}

	let { patches, value = $bindable(), highlight }: Props = $props();

	const patch = $derived(patches.find((p) => p.id === value));

	async function openPatchNotes(target: Patch) {
		if (!target.wikiReference) return;
		try {
			await openUrl(
				`${WIKI_BASE_URL}${encodeURIComponent(target.wikiReference)}?fandom=allow`,
			);
		} catch (error) {
			console.error("Failed to open wiki:", error);
		}
	}
</script>

<div class="flex flex-col gap-2">
	<div class="flex items-baseline justify-between gap-2">
		<span class="text-base-content/60 min-w-0 truncate text-xs">
			{m.wizard_game_version()}
			{#if patch}
				<span class="text-base-content ml-1">
					<span class="font-mono font-semibold">{patch.version}</span> · {patch.name}
				</span>
			{/if}
		</span>
		{#if patch?.wikiReference}
			<button
				type="button"
				class="link flex items-center gap-1 text-xs"
				onclick={() => openPatchNotes(patch)}
			>
				{m.wizard_open_wiki()}
				<ExternalLink size={10} />
			</button>
		{/if}
	</div>
	<PatchTimeline {patches} bind:value label={m.wizard_game_version()} {highlight} />
</div>
