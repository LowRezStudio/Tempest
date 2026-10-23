<script lang="ts">
	import { ArrowLeft, ArrowRight, ExternalLink, Rss } from "@lucide/svelte";
	import Section from "$lib/components/ui/Section.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { BLOG_URL, createBlogFeedQuery } from "$lib/queries/feed";
	import { localeState } from "$lib/stores/locale.svelte";

	const feedQuery = createBlogFeedQuery();
	const posts = $derived((feedQuery.data ?? []).slice(0, 4));
	let activeIndex = $state(0);
	const activePost = $derived(posts[activeIndex]);

	$effect(() => {
		if (activeIndex >= posts.length) activeIndex = 0;
	});

	function move(direction: -1 | 1) {
		if (posts.length < 2) return;
		activeIndex = (activeIndex + direction + posts.length) % posts.length;
	}

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

	function openPost(link: string) {
		openUrl(link);
	}
</script>

<Section title={m.home_news()} onSeeAll={() => openUrl(BLOG_URL)} class="flex h-full flex-col">
	{#if feedQuery.isPending && posts.length === 0}
		<div
			data-panel
			class="bg-base-200 border-base-300 rounded-box flex-1 overflow-hidden border"
			aria-busy="true"
		>
			<div class="bg-base-300 loading-shimmer aspect-[2.4/1] max-h-80"></div>
			<div class="flex flex-col gap-2 p-4">
				<div class="bg-base-300 loading-shimmer h-5 w-2/3 rounded"></div>
				<div class="bg-base-300 loading-shimmer h-3 w-1/2 rounded"></div>
			</div>
		</div>
	{:else if posts.length === 0}
		<p
			data-panel
			class="bg-base-200 border-base-300 rounded-box text-base-content/60 border px-4 py-12 text-center text-sm"
		>
			{m.home_feed_unavailable()}
		</p>
	{:else}
		<div
			data-panel
			class="bg-base-200 border-base-300 rounded-box flex flex-1 flex-col overflow-hidden border"
			role="region"
			aria-roledescription="carousel"
			aria-label={m.home_news()}
		>
			{#if activePost}
				{@const post = activePost}
				<div class="bg-base-300 relative aspect-[2.4/1] max-h-96 overflow-hidden">
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
							<Rss size={32} class="text-base-content/35" />
						</div>
					{/if}
					<div
						class="from-base-200/95 via-base-200/50 absolute inset-0 bg-linear-to-t from-0% via-45% to-transparent"
					></div>
					<div class="absolute right-3 bottom-3 left-4 sm:right-5 sm:bottom-4 sm:left-5">
						<p class="text-base-content/70 mb-1 text-xs font-medium">
							{formatDate(post.pubDate)}
						</p>
						<button
							class="text-left text-lg leading-tight font-bold hover:underline sm:text-2xl"
							onclick={() => openPost(post.link)}
						>
							{post.title}
						</button>
					</div>
					{#if posts.length > 1}
						<div class="absolute top-3 right-3 flex gap-1.5">
							<button
								class="btn btn-sm btn-circle bg-base-100/85 border-base-content/10"
								onclick={() => move(-1)}
								aria-label="Previous article"
							>
								<ArrowLeft size={16} />
							</button>
							<button
								class="btn btn-sm btn-circle bg-base-100/85 border-base-content/10"
								onclick={() => move(1)}
								aria-label="Next article"
							>
								<ArrowRight size={16} />
							</button>
						</div>
					{/if}
				</div>
				<div class="flex items-center justify-end gap-4 p-4">
					<div class="flex shrink-0 items-center gap-3">
						{#if posts.length > 1}
							<div class="flex items-center gap-1.5" aria-label="Choose article">
								{#each posts as item, i (item.link)}
									<button
										class={i === activeIndex
											? "bg-primary h-2 w-5 rounded-full transition-all"
											: "bg-base-content/25 h-2 w-2 rounded-full transition-all"}
										onclick={() => (activeIndex = i)}
										aria-label={`Show article ${i + 1} of ${posts.length}`}
										aria-current={i === activeIndex ? "true" : undefined}
									></button>
								{/each}
							</div>
						{/if}
						<button class="btn btn-ghost btn-sm" onclick={() => openPost(post.link)}>
							Read
							<ExternalLink size={14} />
						</button>
					</div>
				</div>
			{/if}
		</div>
	{/if}
</Section>
