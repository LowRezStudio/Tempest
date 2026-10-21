<script lang="ts">
	import { BadgeCheck, Download, ExternalLink, LifeBuoy, ShieldAlert } from "@lucide/svelte";
	import { createQuery } from "@tanstack/svelte-query";
	import { marked } from "marked";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { getRepoMod, getRepoVersions } from "$lib/queries/repo";
	import { compareVersions, describeCompat, isCompatible } from "$lib/utils/version";
	import BugReportModal from "./BugReportModal.svelte";
	import type { RepoVersion } from "$lib/queries/repo";

	interface Props {
		open?: boolean;
		modId: string | null;
		gamePath: string;
		patch: string | undefined;
		installedVersion?: string;
		isRunning: boolean;
	}

	let {
		open = $bindable(false),
		modId,
		gamePath,
		patch,
		installedVersion,
		isRunning,
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
	let tab = $state<"readme" | "versions">("readme");
	let bugOpen = $state(false);

	const readmeHtml = $derived(
		modQuery.data?.readme
			? (marked.parse(modQuery.data.readme, { async: false }) as string)
			: "",
	);
	const versions = $derived((versionsQuery.data ?? []).filter((v) => v.status === "approved"));

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

<Modal bind:open title={modQuery.data?.name ?? modId ?? ""} class="max-w-3xl">
	{#if modQuery.isPending}
		<div class="flex h-40 items-center justify-center">
			<span class="loading loading-spinner loading-lg opacity-30"></span>
		</div>
	{:else if modQuery.isError || !modQuery.data}
		<p class="text-error text-sm">{m.repo_unreachable()}</p>
	{:else}
		{@const mod = modQuery.data}
		<div class="flex flex-col gap-4">
			<div class="flex flex-wrap items-center gap-2 text-sm">
				<span class="text-base-content/70">{mod.owner.displayName}</span>
				{#if mod.latest?.signer?.trust === "first-party"}
					<span class="badge badge-primary badge-sm gap-1"
						><BadgeCheck size={12} /> {m.repo_first_party()}</span
					>
				{:else if mod.latest?.signer?.trust === "unknown"}
					<span class="badge badge-warning badge-sm gap-1"
						><ShieldAlert size={12} /> {m.repo_unknown_signer()}</span
					>
				{/if}
				<span class="badge badge-ghost badge-sm">{mod.kind}</span>
				{#each mod.tags as tag (tag)}<span class="badge badge-outline badge-sm">{tag}</span
					>{/each}
				<button class="btn btn-ghost btn-xs ml-auto" onclick={() => (bugOpen = true)}>
					<LifeBuoy size={12} />
					{m.repo_bug_get_help()}
				</button>
				<button class="btn btn-ghost btn-xs" onclick={() => openUrl(mod.url)}>
					<ExternalLink size={12} />
					{m.repo_open_in_browser()}
				</button>
			</div>
			<p class="text-base-content/80 text-sm">{mod.summary}</p>

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
			</div>

			{#if tab === "readme"}
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
