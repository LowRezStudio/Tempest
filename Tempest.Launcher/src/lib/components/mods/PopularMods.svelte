<script lang="ts">
	import { ArrowRight, Download } from "@lucide/svelte";
	import { compatibleWith } from "$lib/mods/browse.svelte";
	import { cardImageOf, formatCount, kindLabel } from "$lib/mods/listing";
	import { pendingInstalls } from "$lib/mods/pendingInstalls.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { createRepoCatalogQuery } from "$lib/queries/repo";
	import RepoModIcon from "./RepoModIcon.svelte";
	import type { RepoMod } from "$lib/queries/repo";

	interface Props {
		gamePath: string;
		patch: string | undefined;
		isRunning: boolean;
		onOpen: (modId: string) => void;
		onSeeAll: () => void;
	}

	let { gamePath, patch, isRunning, onOpen, onSeeAll }: Props = $props();

	const catalogQuery = createRepoCatalogQuery(() => "downloads");
	const picks = $derived(
		(catalogQuery.data ?? []).filter((mod) => compatibleWith(mod, patch)).slice(0, 4),
	);
	const pendingIds = $derived(
		new Set(pendingInstalls.value.filter((p) => p.gamePath === gamePath).map((p) => p.modId)),
	);

	function install(mod: RepoMod) {
		if (!mod.latest || pendingIds.has(mod.id)) return;
		void installFromRepo(gamePath, mod.id, mod.latest.version, mod.name, {
			sha256: mod.latest.sha256,
		});
	}
</script>

{#if picks.length > 0}
	<section class="mt-8">
		<div class="mb-2 flex items-baseline justify-between gap-4">
			<h2 class="text-sm font-semibold">
				{patch ? m.mods_popular({ patch }) : m.mods_popular_any()}
			</h2>
			<button
				class="text-base-content/60 hover:text-base-content flex cursor-pointer items-center gap-1 text-xs font-semibold transition-colors"
				onclick={onSeeAll}
			>
				{m.mods_see_all()}
				<ArrowRight size={12} />
			</button>
		</div>
		<ul class="grid gap-2 @3xl:grid-cols-2">
			{#each picks as mod (mod.id)}
				{@const image = cardImageOf(mod)}
				{@const pending = pendingIds.has(mod.id)}
				<li
					class="rounded-box border-base-300 bg-base-200 hover:bg-base-300/40 relative flex items-center gap-3 border p-2 transition-colors"
				>
					<div
						class="bg-base-300 rounded-field flex aspect-video w-20 shrink-0 items-center justify-center overflow-hidden"
					>
						{#if image}
							<img
								src={image.thumbUrl ?? image.url}
								alt=""
								loading="lazy"
								class="size-full object-cover"
							/>
						{:else}
							<RepoModIcon {mod} size={28} />
						{/if}
					</div>
					<div class="flex min-w-0 flex-1 items-center gap-2">
						<div class="min-w-0 flex-1">
							<button
								class="block max-w-full cursor-pointer truncate text-left text-sm font-semibold after:absolute after:inset-0 hover:underline"
								onclick={() => onOpen(mod.id)}
							>
								{mod.name}
							</button>
							<p class="text-base-content/60 flex items-center gap-2 text-xs">
								<span class="truncate">{kindLabel(mod.kind)}</span>
								<span
									class="flex items-center gap-1"
									title={m.repo_downloads({ count: mod.downloads })}
								>
									<Download size={11} />
									{formatCount(mod.downloads)}
								</span>
							</p>
						</div>
						<span
							class="relative z-10"
							title={isRunning ? m.repo_close_game_first() : ""}
						>
							<button
								class="btn btn-sm btn-square"
								disabled={isRunning || pending}
								aria-label="{m.repo_install()} {mod.name}"
								title="{m.repo_install()} v{mod.latest?.version}"
								onclick={() => install(mod)}
							>
								{#if pending}
									<span class="loading loading-spinner loading-xs"></span>
								{:else}
									<Download size={14} />
								{/if}
							</button>
						</span>
					</div>
				</li>
			{/each}
		</ul>
	</section>
{/if}
