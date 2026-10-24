<script lang="ts">
	import { ChevronLeft, ChevronRight } from "@lucide/svelte";
	import Section from "$lib/components/ui/Section.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { createReleasesQuery } from "$lib/queries/release";
	import { localeState } from "$lib/stores/locale.svelte";

	const releasesQuery = createReleasesQuery();
	const releases = $derived(releasesQuery.data ?? []);
	/** 0 is the newest release */
	let index = $state(0);
	const release = $derived(releases[Math.min(index, releases.length - 1)]);
	const date = $derived.by(() => {
		const published = release?.pub_date ? new Date(release.pub_date) : undefined;
		if (!published || Number.isNaN(published.getTime())) return "";
		return new Intl.DateTimeFormat(localeState.current, { dateStyle: "medium" }).format(
			published,
		);
	});
</script>

<Section
	title={m.home_releasenotes_title()}
	class="flex h-full flex-col"
	linkLabel={m.home_releasenotes_more()}
	onSeeAll={() =>
		openUrl(release?.url ?? "https://github.com/LowRezStudio/Tempest/releases/latest")}
>
	<div data-panel class="bg-base-200 border-base-300 rounded-box flex flex-1 flex-col border p-4">
		{#if releasesQuery.isPending && !release}
			<p class="text-base-content/60 text-sm">{m.common_loading()}</p>
		{:else if release}
			<div class="mb-2 flex items-center gap-2">
				{#if release.version}
					<span class="badge badge-primary badge-sm tabular-nums">v{release.version}</span
					>
				{/if}
				<span class="text-base-content/60 text-xs">{date}</span>
				{#if releases.length > 1}
					<!-- Left goes back in time, like a timeline -->
					<div class="ml-auto flex items-center">
						<button
							class="btn btn-ghost btn-xs btn-square"
							disabled={index >= releases.length - 1}
							title={m.home_releasenotes_older()}
							aria-label={m.home_releasenotes_older()}
							onclick={() => index++}
						>
							<ChevronLeft size={14} />
						</button>
						<button
							class="btn btn-ghost btn-xs btn-square"
							disabled={index === 0}
							title={m.home_releasenotes_newer()}
							aria-label={m.home_releasenotes_newer()}
							onclick={() => index--}
						>
							<ChevronRight size={14} />
						</button>
					</div>
				{/if}
			</div>
			<!-- Laid over its box, so long notes scroll inside the card instead of making the row
			     taller than its neighbours; keyed so another release starts at the top -->
			<div class="relative min-h-40 flex-1">
				{#key release.version}
					<p
						class="text-base-content/80 absolute inset-0 overflow-y-auto text-sm leading-relaxed wrap-break-word whitespace-pre-wrap"
					>
						{release.body}
					</p>
				{/key}
			</div>
		{:else}
			<p class="text-base-content/60 text-sm">{m.home_releasenotes_unavailable()}</p>
		{/if}
	</div>
</Section>
