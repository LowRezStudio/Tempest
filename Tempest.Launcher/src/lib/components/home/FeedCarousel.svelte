<script lang="ts">
	import { ChevronLeft, ChevronRight, Rss } from "@lucide/svelte";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import { m } from "$lib/paraglide/messages";
	import { createBlogFeedQuery } from "$lib/queries/feed";
	import { localeState } from "$lib/stores/locale.svelte";

	const feedQuery = createBlogFeedQuery();
	let posts = $derived(feedQuery.data ?? []);
	let feedLoading = $derived(feedQuery.isPending && posts.length === 0);

	let carouselEl: HTMLElement | undefined = $state();
	let current = $state(0);
	let paused = $state(false);
	let imageFailed = $state<boolean[]>([]);

	function formatFeedDate(pubDate: string): string {
		if (!pubDate) return "";
		const date = new Date(pubDate);
		if (Number.isNaN(date.getTime())) return "";
		return new Intl.DateTimeFormat(localeState.current, { dateStyle: "medium" }).format(date);
	}

	function slideStep(): number {
		const el = carouselEl;
		if (!el || posts.length === 0) return 0;
		return el.scrollWidth / posts.length;
	}

	function goTo(index: number) {
		if (posts.length === 0) return;
		const n = posts.length;
		current = ((index % n) + n) % n;
		carouselEl?.scrollTo({ left: current * slideStep(), behavior: "smooth" });
	}

	function onScroll() {
		const step = slideStep();
		if (step <= 0) return;
		const el = carouselEl;
		if (!el) return;
		current = Math.round(el.scrollLeft / step);
	}

	function markImageFailed(index: number) {
		imageFailed[index] = true;
	}

	$effect(() => {
		const count = posts.length;
		if (count <= 1 || paused) return;
		const timer = setInterval(() => {
			goTo(current + 1);
		}, 5000);
		return () => clearInterval(timer);
	});
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="fixed top-6 left-6 z-40 w-[380px]"
	onmouseenter={() => (paused = true)}
	onmouseleave={() => (paused = false)}
>
	<div
		class="card bg-base-200/85 shadow-xl backdrop-blur-sm transition-[filter] duration-150 hover:brightness-90"
	>
		<div class="card-body gap-2 p-2.5">
			<div class="mb-0.5 flex items-center gap-2">
				<div
					class="bg-base-300 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
				>
					<Rss size={16} class="opacity-60" />
				</div>
				<div class="flex-1">
					<h3 class="text-xs font-bold">{m.home_feed_title()}</h3>
					<p class="text-[10px] opacity-60">{m.home_feed_subtitle()}</p>
				</div>
			</div>

			{#if feedLoading}
				<div class="skeleton aspect-video w-full rounded-lg"></div>
			{:else if posts.length > 0}
				<div class="relative">
					<div
						class="carousel carousel-horizontal w-full snap-x snap-mandatory scroll-smooth"
						bind:this={carouselEl}
						onscroll={onScroll}
					>
						{#each posts as post, i (post.link)}
							<div
								class="carousel-item feed-slide relative aspect-video w-full snap-start overflow-hidden rounded-lg"
							>
								<div
									class="from-primary/20 to-base-300 absolute inset-0 flex items-center justify-center bg-gradient-to-br"
								>
									<Rss size={24} class="opacity-40" />
								</div>
								{#if post.image && !imageFailed[i]}
									<img
										src={post.image}
										alt=""
										class="feed-slide-media absolute inset-0 h-full w-full object-cover"
										loading="lazy"
										decoding="async"
										onerror={() => markImageFailed(i)}
									/>
								{/if}
								<button
									type="button"
									class="absolute inset-0 cursor-pointer"
									aria-label={post.title}
									onclick={() => openUrl(post.link)}
								></button>
								<div
									class="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent p-3 pt-8 text-left"
								>
									<p class="text-xs leading-snug font-bold text-white">
										{post.title}
									</p>
									<p class="text-[10px] text-white/75">
										{formatFeedDate(post.pubDate)}
									</p>
								</div>
							</div>
						{/each}
					</div>

					{#if posts.length > 1}
						<button
							type="button"
							class="btn btn-circle btn-ghost btn-xs bg-base-300/70 absolute top-1/2 left-2 -translate-y-1/2 backdrop-blur-sm"
							aria-label={m.home_feed_prev()}
							onclick={() => goTo(current - 1)}
						>
							<ChevronLeft size={14} />
						</button>
						<button
							type="button"
							class="btn btn-circle btn-ghost btn-xs bg-base-300/70 absolute top-1/2 right-2 -translate-y-1/2 backdrop-blur-sm"
							aria-label={m.home_feed_next()}
							onclick={() => goTo(current + 1)}
						>
							<ChevronRight size={14} />
						</button>
					{/if}
				</div>

				{#if posts.length > 1}
					<div class="flex items-center justify-center gap-1 pt-0.5">
						{#each posts as _, i (i)}
							<button
								type="button"
								class="h-1.5 cursor-pointer rounded-full transition-all duration-150"
								class:w-4={i === current}
								class:w-1.5={i !== current}
								class:bg-primary={i === current}
								class:bg-base-300={i !== current}
								aria-label={`${m.home_feed_go_to()} ${i + 1}`}
								onclick={() => goTo(i)}
							></button>
						{/each}
					</div>
				{/if}
			{:else}
				<p class="py-4 text-center text-xs opacity-60">{m.home_feed_unavailable()}</p>
			{/if}
		</div>
	</div>
</div>

<style>
	.feed-slide-media {
		transition: transform 150ms var(--ease-snappy);
	}

	@media (hover: hover) and (pointer: fine) {
		.feed-slide:hover .feed-slide-media {
			transform: scale(1.05);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.feed-slide-media {
			transition: none;
		}
	}
</style>
