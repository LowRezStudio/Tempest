<script lang="ts">
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { getContrastColor } from "$lib/utils/color";
	import { patchBannerOf } from "$lib/utils/versions";
	import type { ClassValue } from "svelte/elements";

	interface Props {
		version: string | undefined;
		/** The instance's color, for the version chip and the bannerless tile */
		color: string;
		class?: ClassValue;
	}

	let { version, color, class: className }: Props = $props();

	const contrast = $derived(getContrastColor(color));
	const banner = $derived(patchBannerOf(version));

	let loadedSrc = $state<string>();
	let failedSrc = $state<string>();
	function watchImage(img: HTMLImageElement) {
		// Already decoded from the cache: no load event comes.
		if (img.complete && img.naturalWidth > 0) loadedSrc = img.getAttribute("src") ?? undefined;
	}
</script>

<!-- A patch as the library shows it: its banner with the sidebar's version chip, or the chip's
     color with the version when the patch has no banner -->
<div class={["relative", className]}>
	{#if banner && failedSrc !== banner}
		<div
			class="bg-base-300 rounded-field relative aspect-5/2 overflow-hidden"
			class:loading-shimmer={loadedSrc !== banner}
		>
			<img
				src={banner}
				alt=""
				decoding="async"
				use:watchImage
				onload={() => (loadedSrc = banner)}
				onerror={() => (failedSrc = banner)}
				class="absolute inset-0 h-full w-full object-cover transition-opacity duration-300"
				class:opacity-0={loadedSrc !== banner}
			/>
		</div>
		<span
			class="absolute bottom-1 left-1 rounded px-1 text-[10px] leading-4 font-bold tabular-nums shadow-sm"
			style="background-color: {color}; color: {contrast};">{version}</span
		>
	{:else}
		<div
			class="rounded-field flex aspect-5/2 items-center justify-center gap-2"
			style="background-color: {color}; color: {contrast};"
		>
			<PaladinsIcon size={28} color={contrast} />
			{#if version}
				<span class="text-sm font-bold tabular-nums">{version}</span>
			{/if}
		</div>
	{/if}
</div>
