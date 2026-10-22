<script lang="ts">
	import { ExternalLink, Rss } from "@lucide/svelte";
	import Section from "$lib/components/ui/Section.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { BLOG_URL, createBlogFeedQuery } from "$lib/queries/feed";
	import { localeState } from "$lib/stores/locale.svelte";

	const feedQuery = createBlogFeedQuery();
	const posts = $derived((feedQuery.data ?? []).slice(0, 4));

	function formatDate(pubDate: string): string {
		const date = new Date(pubDate);
		if (!pubDate || Number.isNaN(date.getTime())) return "";
		return new Intl.DateTimeFormat(localeState.current, { dateStyle: "medium" }).format(date);
	}

	let failed = $state<Record<string, boolean>>({});
	let loaded = $state<Record<string, boolean>>({});
	function watchImage(img: HTMLImageElement, link: string) {
		// Already decoded from the cache: no load event comes.
		if (img.complete && img.naturalWidth > 0) loaded[link] = true;
	}
</script>

<Section title={m.home_news()} onSeeAll={() => openUrl(BLOG_URL)}>
	{#if feedQuery.isPending && posts.length === 0}
		<ul
			class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border"
			aria-busy="true"
		>
			{#each { length: 3 } as _, i (i)}
				<li class="flex items-center gap-4 p-3">
					<div
						class="bg-base-300 rounded-field loading-shimmer aspect-video w-28 shrink-0"
					></div>
					<div class="flex flex-1 flex-col gap-2">
						<div class="bg-base-300 loading-shimmer h-4 w-48 rounded"></div>
						<div class="bg-base-300 loading-shimmer h-3 w-3/4 rounded"></div>
					</div>
				</li>
			{/each}
		</ul>
	{:else if posts.length === 0}
		<p
			class="bg-base-200 border-base-300 rounded-box text-base-content/60 border px-4 py-12 text-center text-sm"
		>
			{m.home_feed_unavailable()}
		</p>
	{:else}
		<ul class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border">
			{#each posts as post (post.link)}
				<li
					class="hover:bg-base-300/40 relative flex items-center gap-4 p-3 transition-colors"
				>
					<div
						class="bg-base-300 rounded-field relative aspect-video w-28 shrink-0 overflow-hidden"
						class:loading-shimmer={post.image &&
							!failed[post.link] &&
							!loaded[post.link]}
					>
						{#if post.image && !failed[post.link]}
							<img
								src={post.image}
								alt=""
								loading="lazy"
								decoding="async"
								use:watchImage={post.link}
								onload={() => (loaded[post.link] = true)}
								onerror={() => (failed[post.link] = true)}
								class="absolute inset-0 size-full object-cover transition-opacity duration-300"
								class:opacity-0={!loaded[post.link]}
							/>
						{:else}
							<div class="flex size-full items-center justify-center">
								<Rss size={20} class="opacity-40" />
							</div>
						{/if}
					</div>
					<div class="min-w-0 flex-1">
						<!-- The whole row opens the post; the stretched button keeps it one tab stop. -->
						<button
							class="line-clamp-1 cursor-pointer text-left font-semibold after:absolute after:inset-0 hover:underline"
							onclick={() => openUrl(post.link)}
						>
							{post.title}
						</button>
						{#if post.description}
							<p class="text-base-content/70 line-clamp-1 text-sm">
								{post.description}
							</p>
						{/if}
						<p class="text-base-content/60 mt-1 text-xs">{formatDate(post.pubDate)}</p>
					</div>
					<ExternalLink size={14} class="text-base-content/40 shrink-0" />
				</li>
			{/each}
		</ul>
	{/if}
</Section>
