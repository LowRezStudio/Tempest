<script lang="ts">
	import HomeCommunity from "$lib/components/home/HomeCommunity.svelte";
	import HomeHero from "$lib/components/home/HomeHero.svelte";
	import HomeNews from "$lib/components/home/HomeNews.svelte";
	import HomeReleaseNotes from "$lib/components/home/HomeReleaseNotes.svelte";
	import { lastLaunchedInstance, orderedInstances } from "$lib/stores/instance.svelte";
	import type { Instance } from "$lib/types/instance";

	const playable = (instance: Instance) =>
		instance.state.type === "prepared" || instance.state.type === "setup";

	// The last launched instance, else the last played one, else the first in library order.
	const featured = $derived.by(() => {
		const last = lastLaunchedInstance.value;
		if (last && playable(last)) return last;
		let best: Instance | undefined;
		for (const instance of orderedInstances.value.filter(playable)) {
			if (!best || (instance.lastPlayedAt ?? 0) > (best.lastPlayedAt ?? 0)) best = instance;
		}
		return best;
	});
</script>

<!-- After the mod website's landing page: a hero with the way in, then what's new -->
<div class="h-full">
	<div class="@container h-full scrollbar-gutter-stable overflow-y-auto">
		<!-- The hero takes whatever height the cards leave, so the page fits the window -->
		<div class="flex min-h-full w-full flex-col gap-6 py-6 pr-2 pl-6">
			<div class="flex min-w-0 flex-1 flex-col gap-4">
				{#key featured?.id}
					<HomeHero instance={featured} instances={orderedInstances.value} />
				{/key}
			</div>
			<div class="grid items-stretch gap-4 @4xl:grid-cols-3">
				<HomeNews />
				<HomeReleaseNotes />
				<HomeCommunity />
			</div>
		</div>
	</div>
</div>
