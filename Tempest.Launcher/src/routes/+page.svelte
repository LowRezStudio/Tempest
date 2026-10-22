<script lang="ts">
	import HomeCommunity from "$lib/components/home/HomeCommunity.svelte";
	import HomeGreeting from "$lib/components/home/HomeGreeting.svelte";
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
	const others = $derived(
		orderedInstances.value.filter((instance) => instance.id !== featured?.id),
	);
</script>

<!-- After the mod website's landing page: a hero with the way in, then what's new -->
<div class="bg-base-100 h-full">
	<div class="@container h-full scrollbar-gutter-stable overflow-y-auto">
		<div class="mx-auto flex w-full max-w-6xl flex-col gap-8 px-4 py-6">
			<div class="flex flex-col gap-4">
				<HomeGreeting instance={featured} instances={orderedInstances.value} />
				{#key featured?.id}
					<HomeHero instance={featured} {others} />
				{/key}
			</div>
			<div class="grid items-start gap-x-4 gap-y-8 @4xl:grid-cols-[minmax(0,1fr)_20rem]">
				<HomeNews />
				<div class="flex flex-col gap-8">
					<HomeReleaseNotes />
					<HomeCommunity />
				</div>
			</div>
		</div>
	</div>
</div>
