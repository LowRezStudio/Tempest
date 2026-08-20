import { createQuery } from "@tanstack/svelte-query";
import { fetch } from "@tauri-apps/plugin-http";
import { parseFeed } from "feedsmith";
import { persistedState } from "$lib/stores/persisted.svelte";

const BLOG_FEED_URL = "https://docs.lowrezstudio.com/rss.xml";

export type BlogPost = {
	title: string;
	link: string;
	pubDate: string;
	image?: string;
};

/** localStorage cache so the feed renders instantly and stays visible offline. */
export const cachedFeed = persistedState<BlogPost[] | null>("homeBlogFeed", null);

export async function fetchBlogFeed(): Promise<BlogPost[] | null> {
	try {
		const controller = new AbortController();
		const timeout = setTimeout(() => controller.abort(), 5000);
		const res = await fetch(BLOG_FEED_URL, { signal: controller.signal });
		clearTimeout(timeout);
		if (!res.ok) return cachedFeed.value;
		const xml = await res.text();
		const result = parseFeed(xml);
		if (result.format !== "rss") return cachedFeed.value;
		const posts: BlogPost[] = (result.feed.items ?? [])
			.filter((item) => item.title && item.link)
			.map((item) => ({
				title: item.title ?? "",
				link: item.link ?? "",
				pubDate: item.pubDate ?? "",
				image: item.media?.thumbnails?.[0]?.url,
			}));
		if (posts.length === 0) return cachedFeed.value;
		cachedFeed.value = posts;
		return posts;
	} catch {
		// Offline or transient failure: fall back to the last known posts.
		return cachedFeed.value;
	}
}

export const createBlogFeedQuery = () =>
	createQuery(() => ({
		queryKey: ["blog-feed"],
		queryFn: fetchBlogFeed,
		placeholderData: () => cachedFeed.value ?? undefined,
		staleTime: 15 * 60 * 1000,
		retry: 1,
	}));
