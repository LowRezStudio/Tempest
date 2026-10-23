<script lang="ts">
	import { Check, Download, ExternalLink, LifeBuoy } from "@lucide/svelte";
	import { createQuery } from "@tanstack/svelte-query";
	import { marked } from "marked";
	import ModFileTree from "$lib/components/mods/ModFileTree.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { kindLabel } from "$lib/mods/listing";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { createAboutInfoQuery } from "$lib/queries/about";
	import { getRepoMod, getRepoVersions } from "$lib/queries/repo";
	import {
		describeRequirement,
		modDependencies,
		tempestRequirement,
		satisfies,
	} from "$lib/utils/deps";
	import { compareVersions, describeCompat, isCompatible } from "$lib/utils/version";
	import BugReportModal from "./BugReportModal.svelte";
	import RepoModIcon from "./RepoModIcon.svelte";
	import RepoTrustBadge from "./RepoTrustBadge.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { RepoVersion } from "$lib/queries/repo";

	interface Props {
		open?: boolean;
		modId: string | null;
		gamePath: string;
		patch: string | undefined;
		installedVersion?: string;
		isRunning: boolean;
		installedMod?: ModRecord;
	}

	let {
		open = $bindable(false),
		modId,
		gamePath,
		patch,
		installedVersion,
		isRunning,
		installedMod,
	}: Props = $props();

	const modQuery = createQuery(() => ({
		queryKey: ["repo", "mod", modId],
		queryFn: () => getRepoMod(modId!),
		enabled: open && !!modId,
		staleTime: 60_000,
	}));
	const versionsQuery = createQuery(() => ({
		queryKey: ["repo", "versions", modId],
		queryFn: () => getRepoVersions(modId!),
		enabled: open && !!modId,
		staleTime: 60_000,
	}));

	let installing = $state<string | null>(null);
	let tab = $state<"readme" | "versions" | "files">("readme");
	let bugOpen = $state(false);

	const readmeHtml = $derived(
		modQuery.data?.readme
			? (marked.parse(modQuery.data.readme, { async: false }) as string)
			: "",
	);
	const versions = $derived((versionsQuery.data ?? []).filter((v) => v.status === "approved"));
	// The newest version for this patch: what the Overview offers to install.
	const primaryVersion = $derived(versions.find((v) => isCompatible(v.compat, patch)) ?? null);
	const screenshots = $derived(modQuery.data?.latest?.assets?.screenshots ?? []);
	// What the offered version needs: other mods (installed alongside) and a launcher version.
	const about = createAboutInfoQuery();
	const requires = $derived(modDependencies(primaryVersion?.dependencies ?? []));
	const needsTempest = $derived(
		primaryVersion ? tempestRequirement(primaryVersion.dependencies ?? []) : undefined,
	);
	const tempestTooOld = $derived(
		needsTempest !== undefined &&
			!!about.data?.appVersion &&
			!satisfies(about.data.appVersion, needsTempest),
	);
	let shotIndex = $state(0);
	$effect(() => {
		void modId;
		shotIndex = 0;
		tab = "readme";
	});

	function action(v: RepoVersion): "installed" | "update" | "downgrade" | "install" {
		if (!installedVersion) return "install";
		const d = compareVersions(v.version, installedVersion);
		return d === 0 ? "installed" : d > 0 ? "update" : "downgrade";
	}

	async function install(v: RepoVersion) {
		if (installing || !modId) return;
		installing = v.version;
		try {
			await installFromRepo(gamePath, modId, v.version, modQuery.data?.name ?? modId, {
				replace: !!installedVersion,
			});
		} finally {
			installing = null;
		}
	}

	const fmtSize = (n: number) =>
		n < 1024 ** 2 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1024 ** 2).toFixed(1)} MB`;
</script>

<Modal
	bind:open
	title={modQuery.data?.name ?? modId ?? ""}
	class="h-[85vh] max-w-3xl scrollbar-gutter-stable"
>
	{#if modQuery.isPending}
		<div class="flex h-40 items-center justify-center">
			<span class="loading loading-spinner loading-lg opacity-30"></span>
		</div>
	{:else if modQuery.isError || !modQuery.data}
		<p class="text-error text-sm">{m.repo_unreachable()}</p>
	{:else}
		{@const mod = modQuery.data}
		{@const primary = primaryVersion}
		<div class="flex flex-col gap-4">
			<div class="flex items-start gap-3">
				<RepoModIcon {mod} size={48} />
				<div class="flex min-w-0 flex-1 flex-col gap-1.5">
					<p class="text-base-content/80 text-sm">{mod.summary}</p>
					<div class="flex flex-wrap items-center gap-1.5 text-sm">
						<span class="text-base-content/70 mr-1 flex items-center gap-1.5">
							{#if mod.owner.avatar}
								<img src={mod.owner.avatar} alt="" class="size-4 rounded-full" />
							{/if}
							{mod.owner.displayName}
						</span>
						<RepoTrustBadge latest={mod.latest} />
						<span class="badge badge-ghost badge-sm">{kindLabel(mod.kind)}</span>
						{#each mod.tags as tag (tag)}<span class="badge badge-ghost badge-sm"
								>#{tag}</span
							>{/each}
					</div>
				</div>
				{#if primary}
					{@const what = action(primary)}
					{#if tempestTooOld}
						<span
							class="badge badge-warning shrink-0"
							title={m.deps_needs_tempest({
								requirement: needsTempest ?? "",
								version: about.data?.appVersion ?? "",
							})}
							>{m.repo_needs_newer_tempest({ requirement: needsTempest ?? "" })}</span
						>
					{:else if what === "installed"}
						<span
							class="text-success flex shrink-0 items-center gap-1.5 text-sm font-semibold"
						>
							<Check size={16} />
							{m.repo_installed()}
						</span>
					{:else}
						<button
							class="btn btn-primary btn-sm shrink-0"
							disabled={isRunning || installing !== null}
							title={isRunning ? m.repo_close_game_first() : `v${primary.version}`}
							onclick={() => install(primary)}
						>
							{#if installing === primary.version}<span
									class="loading loading-spinner loading-xs"
								></span>{:else}<Download size={14} />{/if}
							{what === "update"
								? m.repo_update_available({ version: primary.version })
								: m.repo_install()}
						</button>
					{/if}
				{/if}
			</div>
			{#if requires.length > 0 || needsTempest !== undefined}
				<p class="text-base-content/70 flex flex-wrap items-center gap-1.5 text-xs">
					<span class="font-semibold">{m.repo_requires()}</span>
					{#if needsTempest !== undefined}
						<span class="badge badge-ghost badge-sm font-mono"
							>{m.repo_requires_tempest({
								requirement: describeRequirement(needsTempest),
							})}</span
						>
					{/if}
					{#each requires as dep (dep.id)}
						<span class="badge badge-ghost badge-sm font-mono"
							>{dep.id} {describeRequirement(dep.requirement)}</span
						>
					{/each}
				</p>
			{/if}
			<div class="flex items-center gap-1">
				<button class="btn btn-ghost btn-xs" onclick={() => (bugOpen = true)}>
					<LifeBuoy size={12} />
					{m.repo_bug_get_help()}
				</button>
				<button class="btn btn-ghost btn-xs" onclick={() => openUrl(mod.url)}>
					<ExternalLink size={12} />
					{m.repo_open_in_browser()}
				</button>
			</div>

			<div role="tablist" class="tabs tabs-border">
				<button
					role="tab"
					class="tab"
					class:tab-active={tab === "readme"}
					onclick={() => (tab = "readme")}>{m.repo_tab_readme()}</button
				>
				<button
					role="tab"
					class="tab"
					class:tab-active={tab === "versions"}
					onclick={() => (tab = "versions")}
				>
					{m.repo_tab_versions({ count: versions.length })}
				</button>
				{#if installedMod}
					<button
						role="tab"
						class="tab"
						class:tab-active={tab === "files"}
						onclick={() => (tab = "files")}
					>
						{m.mod_details_tab_files({
							count: installedMod.InstalledFiles?.length ?? 0,
						})}
					</button>
				{/if}
			</div>

			{#if tab === "files" && installedMod}
				<ModFileTree
					files={installedMod.InstalledFiles}
					basePath={gamePath}
					modId={installedMod.Id}
					ownedFiles={installedMod.OwnedFiles}
				/>
			{:else if tab === "readme"}
				{#if screenshots.length > 0}
					{@const shot = screenshots[Math.min(shotIndex, screenshots.length - 1)]!}
					<figure class="flex flex-col gap-2">
						<div
							class="bg-base-300 rounded-box relative aspect-video w-full overflow-hidden"
						>
							<!-- Keyed, so the previous picture never sits under the new caption while loading. -->
							{#key shot.url}
								<img
									src={shot.url}
									alt={shot.caption ?? ""}
									class="absolute inset-0 h-full w-full object-contain"
								/>
							{/key}
						</div>
						{#if shot.caption}
							<figcaption class="text-base-content/60 text-xs">
								{shot.caption}
							</figcaption>
						{/if}
						{#if screenshots.length > 1}
							<div class="flex gap-2 overflow-x-auto pb-1">
								{#each screenshots as s, i (s.url)}
									<button
										class="rounded-field aspect-video w-24 shrink-0 cursor-pointer overflow-hidden border-2 transition-opacity {i ===
										shotIndex
											? 'border-primary'
											: 'border-transparent opacity-60 hover:opacity-100'}"
										aria-label={s.caption || `${i + 1}`}
										aria-current={i === shotIndex ? "true" : undefined}
										onclick={() => (shotIndex = i)}
									>
										<img
											src={s.thumbUrl ?? s.url}
											alt=""
											class="h-full w-full object-cover"
										/>
									</button>
								{/each}
							</div>
						{/if}
					</figure>
				{/if}
				<article class="prose prose-sm max-w-none">{@html readmeHtml}</article>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each versions as v (v.id)}
						{@const ok = isCompatible(v.compat, patch)}
						{@const what = action(v)}
						<li class="bg-base-200 rounded-lg p-3" class:opacity-60={!ok}>
							<div class="flex flex-wrap items-center gap-2">
								<span class="font-mono font-semibold">v{v.version}</span>
								{#if what === "installed"}<span class="badge badge-success badge-xs"
										>{m.repo_installed()}</span
									>{/if}
								{#if !ok}<span class="badge badge-warning badge-xs"
										>{m.repo_incompatible({ patch: patch ?? "" })}</span
									>{/if}
								<span class="text-base-content/60 text-xs"
									>{fmtSize(v.size)} · {v.publishedAt
										? new Date(v.publishedAt).toLocaleDateString()
										: ""} · {describeCompat(v.compat)}</span
								>
								{#if what !== "installed"}
									<button
										class="btn btn-xs ml-auto {what === 'update'
											? 'btn-primary'
											: what === 'downgrade'
												? 'btn-outline'
												: 'btn-primary'}"
										disabled={isRunning || installing !== null || !ok}
										onclick={() => install(v)}
									>
										{#if installing === v.version}<span
												class="loading loading-spinner loading-xs"
											></span>{:else}<Download size={12} />{/if}
										{what === "update"
											? m.repo_update()
											: what === "downgrade"
												? m.repo_downgrade()
												: m.repo_install()}
									</button>
								{/if}
							</div>
							{#if v.changelog}<p
									class="text-base-content/80 mt-1 text-sm whitespace-pre-line"
								>
									{v.changelog}
								</p>{/if}
						</li>
					{:else}
						<li class="text-base-content/60 text-sm">
							{m.repo_no_published_version()}
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{/if}
</Modal>

<BugReportModal
	bind:open={bugOpen}
	{modId}
	versions={versions.map((v) => v.version)}
	defaultVersion={installedVersion ?? ""}
	defaultGameVersion={patch ?? ""}
/>
