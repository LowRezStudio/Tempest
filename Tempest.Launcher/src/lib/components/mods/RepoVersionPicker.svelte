<script lang="ts">
	import { ArrowRight, ChevronDown, Download, HardDrive } from "@lucide/svelte";
	import { createQuery } from "@tanstack/svelte-query";
	import { Popover } from "bits-ui";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { createRepoVersionsQuery } from "$lib/queries/repo";
	import { compareVersions, describeCompat, isCompatible } from "$lib/utils/version";
	import type { RepoMod, RepoUpdate, RepoVersion } from "$lib/queries/repo";

	interface Props {
		mod: RepoMod;
		installedVersion: string;
		update?: RepoUpdate;
		patch?: string;
		/** Game running or an install of this mod in flight. */
		disabled?: boolean;
		onInstall: (version: RepoVersion) => void;
	}

	let { mod, installedVersion, update, patch, disabled = false, onInstall }: Props = $props();

	const SHOWN = 3;

	let open = $state(false);
	let showOlder = $state(false);
	const versionsQuery = createRepoVersionsQuery(
		() => mod.id,
		() => open,
	);
	const cachedQuery = createQuery(() => ({
		queryKey: ["mod-cache", mod.id],
		queryFn: () => invoke("mods:cached-versions", { modId: mod.id }),
		enabled: open,
	}));
	const versions = $derived((versionsQuery.data ?? []).filter((v) => v.status === "approved"));
	const cached = $derived(new Set(cachedQuery.data ?? []));
	const updateTo = $derived(update?.updateAvailable ? update.latest : null);
	const visible = $derived(showOlder ? versions : versions.slice(0, SHOWN));

	const formatDate = (iso: string) =>
		new Date(iso).toLocaleDateString("en-GB", {
			day: "numeric",
			month: "short",
			year: "numeric",
		});
	const formatBytes = (n: number) =>
		n < 1024 ** 2 ? `${Math.round(n / 1024)} KB` : `${(n / 1024 ** 2).toFixed(1)} MB`;
	const patchLabel = (v: RepoVersion) => {
		const d = describeCompat(v.compat);
		return d === "all" ? m.repo_any_patch() : m.repo_patch({ patch: d || "–" });
	};

	function choose(version: RepoVersion) {
		open = false;
		onInstall(version);
	}
</script>

<Popover.Root bind:open onOpenChange={(o) => !o && (showOlder = false)}>
	<Popover.Trigger
		class="btn btn-ghost btn-xs -ml-2 gap-1 text-sm font-semibold {updateTo
			? 'text-primary'
			: ''}"
		title={updateTo
			? m.repo_update_available({ version: updateTo.version })
			: m.repo_versions()}
	>
		{installedVersion || m.common_unknown()}
		{#if updateTo}
			<ArrowRight size={12} class="opacity-70" />
			{updateTo.version}
		{/if}
		<ChevronDown size={12} class="opacity-50" />
	</Popover.Trigger>
	<Popover.Portal>
		<Popover.Content
			data-popover-menu
			class="bg-base-200 border-base-300 rounded-box popover-animate z-50 w-80 border p-3 text-sm shadow-xl"
			align="start"
			sideOffset={6}
		>
			<h3 class="mb-3 font-semibold">
				{m.repo_versions()}
				{#if versions.length > 0}
					<span class="text-base-content/60 font-normal">({versions.length})</span>
				{/if}
			</h3>
			{#if updateTo}
				<button
					class="btn btn-primary btn-sm mb-3 w-full"
					{disabled}
					onclick={() => choose(updateTo)}
				>
					<Download size={14} />
					{m.repo_update_available({ version: updateTo.version })}
				</button>
			{/if}
			{#if versionsQuery.isPending}
				<div class="flex justify-center py-4">
					<span class="loading loading-spinner loading-sm opacity-50"></span>
				</div>
			{:else if versionsQuery.isError}
				<p class="text-error text-xs">{m.repo_unreachable()}</p>
			{:else}
				{@const solo = versions.length === 1}
				<ol class="flex max-h-80 flex-col gap-1 overflow-y-auto" class:pl-1.5={!solo}>
					{#each visible as v, i (v.id)}
						{@const latest = i === 0}
						{@const oldest = v === versions.at(-1)}
						{@const current = compareVersions(v.version, installedVersion) === 0}
						{@const ok = isCompatible(v.compat, patch)}
						<!-- Same timeline as the website's version list: the rail in two halves around
						     the dot, so it starts at the newest and stops at the oldest. -->
						{@const at = latest ? "top-1/3" : oldest ? "top-2/3" : "top-1/2"}
						<li class="relative" class:pl-4={!solo}>
							{#if !solo && !latest}
								<span
									class="bg-base-300 absolute -top-0.5 -left-px w-0.5 {oldest
										? 'bottom-1/3'
										: 'bottom-1/2'}"
									aria-hidden="true"
								></span>
							{/if}
							{#if !solo && !oldest}
								<span
									class="bg-base-300 absolute -bottom-0.5 -left-px w-0.5 {at}"
									aria-hidden="true"
								></span>
							{/if}
							{#if !solo}
								<span
									class="absolute -left-1.5 size-3 -translate-y-1/2 rounded-full border-2 {at} {current
										? 'bg-primary border-primary'
										: latest
											? 'border-primary bg-base-200'
											: 'border-base-content/30 bg-base-200'}"
									aria-hidden="true"
								></span>
							{/if}
							<button
								class="rounded-field block w-full px-2 py-1.5 text-left transition-colors {current
									? 'bg-primary/10 cursor-default'
									: ok && !disabled
										? 'hover:bg-base-300/60 cursor-pointer'
										: 'cursor-not-allowed opacity-50'}"
								disabled={current || !ok || disabled}
								aria-current={current ? "true" : undefined}
								onclick={() => choose(v)}
							>
								<!-- Never wraps, so every entry has the same height whatever the version length. -->
								<span class="flex min-w-0 items-center gap-2">
									<span
										class="truncate font-mono font-semibold"
										title="v{v.version}">v{v.version}</span
									>
									{#if latest}
										<span class="badge badge-primary badge-xs shrink-0"
											>{m.repo_latest()}</span
										>
									{/if}
									{#if current}
										<span class="badge badge-success badge-xs shrink-0"
											>{m.repo_installed()}</span
										>
									{:else if !ok}
										<span class="badge badge-warning badge-xs shrink-0"
											>{m.repo_incompatible({ patch: patch ?? "" })}</span
										>
									{/if}
								</span>
								<span
									class="text-base-content/60 flex items-center gap-1 text-xs whitespace-nowrap"
								>
									{#if v.publishedAt}{formatDate(v.publishedAt)} ·{/if}
									{patchLabel(v)} · {formatBytes(v.size)}
									{#if cached.has(v.version) && !current}
										· <HardDrive size={10} />
										{m.repo_cached()}
									{/if}
								</span>
								{#if v.changelog}
									<!-- No `block`: it overrides the clamp's -webkit-box and shows it all -->
									<span
										class="text-base-content/80 mt-0.5 line-clamp-2"
										title={v.changelog}>{v.changelog}</span
									>
								{/if}
							</button>
						</li>
					{/each}
				</ol>
				{#if versions.length > SHOWN && !showOlder}
					<button
						class="link mt-2 text-xs no-underline hover:underline"
						onclick={() => (showOlder = true)}
					>
						{m.repo_show_older({ count: versions.length - SHOWN })}
					</button>
				{/if}
			{/if}
		</Popover.Content>
	</Popover.Portal>
</Popover.Root>
