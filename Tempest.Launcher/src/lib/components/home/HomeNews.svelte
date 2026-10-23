<script lang="ts">
	import { ChevronLeft, ChevronRight, ExternalLink, Rss } from "@lucide/svelte";
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

	// Full width the frame keeps a banner ratio; in the three-column row it fills the row.
	const frame =
		"rounded-box border-base-300 bg-base-200 relative isolate flex aspect-[2.4/1] max-h-96 min-h-64 flex-col overflow-hidden border @4xl:aspect-auto @4xl:max-h-none @4xl:flex-1";
</script>

<Section title={m.home_news()} onSeeAll={() => openUrl(BLOG_URL)} class="flex h-full flex-col">
	{#if feedQuery.isPending && posts.length === 0}
		<div data-panel class={[frame, "loading-shimmer justify-end gap-2 p-5"]} aria-busy="true">
			<div class="bg-base-content/10 h-3 w-24 rounded"></div>
			<div class="bg-base-content/10 h-6 w-2/3 rounded"></div>
		</div>
	{:else if posts.length === 0}
		<div data-panel class={[frame, "items-center justify-center gap-2 p-5 text-center"]}>
			<Rss size={24} class="text-base-content/35" />
			<p class="text-base-content/60 text-sm">{m.home_feed_unavailable()}</p>
		</div>
	{:else if activePost}
		{@const post = activePost}
		<div
			data-panel
			class={[frame, "group justify-end"]}
			role="region"
			aria-roledescription="carousel"
			aria-label={m.home_news()}
		>
			<!-- Every slide stays mounted so switching crossfades without a reload -->
			{#each posts as item, i (item.link)}
				{#if item.image && !failed[item.link]}
					<img
						src={item.image}
						alt=""
						draggable="false"
						decoding="async"
						use:watchImage={item.link}
						onload={() => (loaded[item.link] = true)}
						onerror={() => (failed[item.link] = true)}
						class={[
							"pointer-events-none absolute inset-0 -z-10 size-full object-cover transition duration-500 select-none group-hover:scale-[1.03]",
							(i !== activeIndex || !loaded[item.link]) && "opacity-0",
						]}
					/>
				{/if}
			{/each}
			{#if !post.image || failed[post.link]}
				<div class="absolute inset-0 -z-10 flex items-center justify-center pb-16">
					<Rss size={40} class="text-base-content/20" />
				</div>
			{/if}
			<!-- Keeps the text readable over any image, in any theme -->
			<div
				class="from-base-200 via-base-200/75 absolute inset-0 -z-10 bg-linear-to-t from-10% via-40% to-transparent to-75%"
			></div>

			<button
				class="peer rounded-box focus-visible:outline-primary absolute inset-0 cursor-pointer focus-visible:outline-2 focus-visible:-outline-offset-2"
				aria-label={post.title}
				onclick={() => openUrl(post.link)}
			></button>

			{#if posts.length > 1}
				<div
					class="bg-base-100/60 rounded-field absolute top-3 right-3 flex items-center gap-0.5 p-0.5 text-xs backdrop-blur-sm"
				>
					<button
						class="btn btn-ghost btn-xs btn-square"
						aria-label="Previous article"
						onclick={() => move(-1)}
					>
						<ChevronLeft size={14} />
					</button>
					<span class="text-base-content/70 min-w-8 text-center tabular-nums">
						{activeIndex + 1} / {posts.length}
					</span>
					<button
						class="btn btn-ghost btn-xs btn-square"
						aria-label="Next article"
						onclick={() => move(1)}
					>
						<ChevronRight size={14} />
					</button>
				</div>
			{/if}

			<div
				class="pointer-events-none relative flex flex-col gap-1 p-5 peer-hover:[&_h3]:underline peer-focus-visible:[&_h3]:underline"
			>
				<p class="text-base-content/70 flex items-center gap-1.5 text-xs font-medium">
					{formatDate(post.pubDate)}
					<ExternalLink
						size={12}
						class="opacity-0 transition-opacity group-hover:opacity-100"
					/>
				</p>
				<h3 class="line-clamp-2 text-xl leading-tight font-bold tracking-tight">
					{post.title}
				</h3>
				{#if post.description}
					<p class="text-base-content/70 line-clamp-2 text-sm">{post.description}</p>
				{/if}
			</div>
		</div>
	{/if}
</Section>
