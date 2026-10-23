<script lang="ts">
	import Section from "$lib/components/ui/Section.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { createLatestReleaseQuery } from "$lib/queries/release";
	import { localeState } from "$lib/stores/locale.svelte";

	const releaseQuery = createLatestReleaseQuery();
	const release = $derived(releaseQuery.data);
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
	linkLabel={m.home_releasenotes_more()}
	onSeeAll={() =>
		openUrl(release?.url ?? "https://github.com/LowRezStudio/Tempest/releases/latest")}
>
	<div data-panel class="bg-base-200 border-base-300 rounded-box border p-4">
		{#if releaseQuery.isPending && !release}
			<p class="text-base-content/60 text-sm">{m.common_loading()}</p>
		{:else if release}
			<div class="mb-2 flex items-center gap-2">
				{#if release.version}
					<span class="badge badge-primary badge-sm tabular-nums">v{release.version}</span
					>
				{/if}
				<span class="text-base-content/60 text-xs">{date}</span>
			</div>
			<p
				class="text-base-content/80 line-clamp-8 text-sm leading-relaxed whitespace-pre-wrap"
			>
				{release.body}
			</p>
		{:else}
			<p class="text-base-content/60 text-sm">{m.home_releasenotes_unavailable()}</p>
		{/if}
	</div>
</Section>
