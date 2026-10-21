<script lang="ts">
	import {
		BadgeCheck,
		Box,
		ExternalLink,
		FolderOpen,
		Info,
		PackageX,
		RefreshCw,
		Trash2,
	} from "@lucide/svelte";
	import { SvelteSet } from "svelte/reactivity";
	import { justInstalled, pendingInstalls } from "$lib/mods/pendingInstalls.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openUrl } from "$lib/platform/api";
	import {
		createRenameModMutation,
		createEnableModMutation,
		createDisableModMutation,
	} from "$lib/queries/mods";
	import { createRepoModsQuery } from "$lib/queries/repo";
	import { addToast } from "$lib/stores/ui.svelte";
	import RepoVersionPicker from "./RepoVersionPicker.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";
	import type { RepoUpdate } from "$lib/queries/repo";

	interface Props {
		mods: ModRecord[];
		gamePath: string;
		/** The instance's game patch, for the repository's compatibility info. */
		patch?: string;
		isRunning: boolean;
		isLoading: boolean;
		onRefresh: () => void;
		onRemoveMod: (name: string) => void;
		onOpenDetails: (mod: ModRecord) => void;
		/** Repository update checks keyed by mod id */
		updates?: Map<string, RepoUpdate>;
		onOpenRepo?: (mod: ModRecord) => void;
	}

	let {
		mods,
		gamePath,
		patch,
		isRunning,
		isLoading,
		onRefresh,
		onRemoveMod,
		onOpenDetails,
		updates = new Map(),
		onOpenRepo,
	}: Props = $props();

	// Looked up per installed V2 mod (plain GETs; unknown ids come back empty), so the repo
	// features don't depend on the update check, which only adds the update info.
	const repoMods = createRepoModsQuery(() =>
		mods.filter((mod) => mod.Kind === "V2").map((mod) => mod.Id),
	);

	function installVersion(mod: ModRecord, version: string, sha256: string) {
		void installFromRepo(gamePath, mod.Id, version, mod.Name, { replace: true, sha256 });
	}

	async function showInExplorer(mod: ModRecord) {
		if (mod.Kind === "V2") {
			if (!gamePath) return;
		} else if (!(mod.InstalledFiles[0] || mod.OriginalPath || "")) return;
		// Host resolves the folder (V2 mod dir / V1 file's parent) and opens it.
		await invoke("mods:open-location", {
			gamePath,
			mod:
				mod.Kind === "V2"
					? { kind: "V2", modId: mod.Id }
					: { kind: "V1", targetPath: mod.InstalledFiles[0] || mod.OriginalPath || "" },
		});
	}

	const pending = $derived(pendingInstalls.value.filter((p) => p.gamePath === gamePath));
	// Installs of a mod that is already listed (updates, version switches) play out in its own
	// row; the rest get a placeholder row.
	const pendingByMod = $derived(
		new Map(pending.flatMap((p) => (p.modId ? [[p.modId, p] as const] : []))),
	);
	const placeholders = $derived(
		pending.filter((p) => !p.modId || !mods.some((mod) => mod.Id === p.modId)),
	);
	const statusOf = (install: PendingInstall) =>
		install.progress === null
			? m.mod_row_installing()
			: `${Math.round(install.progress * 100)}%`;
	const progressStyle = (install: PendingInstall) =>
		`--progress: ${install.progress === null ? 100 : install.progress * 100}%`;

	const renameMutation = createRenameModMutation();
	const enableMutation = createEnableModMutation();
	const disableMutation = createDisableModMutation();
	const togglingModIds = new SvelteSet<string>();
	let renamingModId = $state<string | null>(null);
	let editName = $state("");

	const errorMessage = (error: unknown) =>
		error instanceof Error ? error.message : String(error);

	function startRename(mod: ModRecord) {
		renamingModId = mod.Id;
		editName = mod.Name;
	}

	async function commitRename(mod: ModRecord) {
		if (renamingModId !== mod.Id) return;
		// Cleared first: removing the input fires blur, which would commit again.
		renamingModId = null;
		const newName = editName.trim();
		if (!newName || newName === mod.Name) return;
		try {
			await renameMutation.mutateAsync({ gamePath, oldName: mod.Name, newName });
		} catch (error) {
			addToast({
				title: m.mod_rename_dialog_title(),
				message: errorMessage(error),
				tone: "error",
			});
		}
	}

	function handleRenameKey(e: KeyboardEvent, mod: ModRecord) {
		if (e.key === "Enter") {
			e.preventDefault();
			void commitRename(mod);
		} else if (e.key === "Escape") {
			e.preventDefault();
			renamingModId = null;
		}
	}

	function focusAndSelect(node: HTMLInputElement) {
		node.focus();
		node.select();
	}

	// The list updates optimistically; a second click while the CLI still works on
	// the same mod is ignored so two enable/disable runs never race.
	const handleToggle = async (mod: ModRecord, input: HTMLInputElement) => {
		if (togglingModIds.has(mod.Id)) {
			input.checked = mod.Enabled;
			return;
		}
		togglingModIds.add(mod.Id);
		try {
			await (mod.Enabled ? disableMutation : enableMutation).mutateAsync({
				gamePath,
				modName: mod.Name,
			});
		} catch (error) {
			addToast({ message: `${mod.Name}: ${errorMessage(error)}`, tone: "error" });
		} finally {
			togglingModIds.delete(mod.Id);
		}
	};
</script>

<table class="table">
	<thead>
		<tr>
			<th>
				<button class="flex items-center gap-1 text-sm font-semibold">
					<span>{m.common_name()}</span>
				</button>
			</th>
			<th class="w-48">{m.common_version()}</th>
			<th class="w-auto text-right">
				<button
					class="hover:text-primary inline-flex cursor-pointer items-center gap-1 text-sm font-semibold whitespace-nowrap tabular-nums transition-colors disabled:opacity-40"
					onclick={onRefresh}
					disabled={isLoading}
					aria-busy={isLoading}
				>
					<span
						class="flex size-3.5 shrink-0 items-center justify-center"
						aria-hidden="true"
					>
						{#if isLoading}
							<span class="loading loading-spinner size-3.5"></span>
						{:else}
							<RefreshCw size={14} />
						{/if}
					</span>
					{m.mod_refresh_list({ count: mods.length })}
				</button>
			</th>
		</tr>
	</thead>
	<tbody>
		{#each mods as mod (mod.Id)}
			{@const repo = repoMods.data?.get(mod.Id)}
			{@const update = updates.get(mod.Id)}
			{@const busy = pendingByMod.get(mod.Id)}
			{@const trust = repo?.latest?.signer?.trust}
			<tr
				class="hover"
				class:just-installed={justInstalled.has(mod.Id)}
				class:pending-row={!!busy}
				class:installing={busy?.progress === null}
				aria-busy={busy ? "true" : undefined}
				style={busy ? progressStyle(busy) : undefined}
			>
				<td class={!mod.Enabled ? "opacity-40" : ""}>
					<div class="flex items-center gap-3">
						<button
							class="bg-base-200 hover:bg-base-300 text-primary flex h-10 w-10 shrink-0 cursor-pointer items-center justify-center overflow-hidden rounded-lg transition-all hover:scale-105 active:scale-95"
							onclick={() => onOpenDetails(mod)}
							title={m.mod_updated_files()}
						>
							{#if busy}
								<span class="loading loading-spinner loading-sm"></span>
							{:else if repo?.icon}
								<img
									src={repo.icon}
									alt=""
									class="h-full w-full object-cover"
									class:grayscale={!mod.Enabled}
								/>
							{:else if mod.Enabled}
								<Box size={20} class="opacity-75" />
							{:else}
								<PackageX size={20} class="opacity-75" />
							{/if}
						</button>
						<div class="min-w-0 flex-1">
							{#if renamingModId === mod.Id}
								<input
									type="text"
									class="input input-xs -mx-1.5 w-full max-w-xs px-1.5 text-sm font-bold"
									aria-label={m.mod_rename_dialog_label()}
									bind:value={editName}
									use:focusAndSelect
									onkeydown={(e) => handleRenameKey(e, mod)}
									onblur={() => commitRename(mod)}
								/>
							{:else}
								<div class="flex min-w-0 items-center">
									<button
										class="hover:bg-base-300 -ml-1.5 block max-w-full cursor-text truncate rounded pr-1 pl-1.5 text-left text-sm font-bold transition-colors"
										onclick={() => startRename(mod)}
										title={m.mod_rename()}
									>
										{mod.Name}
									</button>
									{#if trust === "first-party" || trust === "trusted"}
										<span
											class="flex shrink-0"
											title={trust === "first-party"
												? m.repo_first_party()
												: m.repo_trusted_signer()}
										>
											<BadgeCheck
												size={15}
												class="fill-primary text-base-100"
											/>
										</span>
									{/if}
								</div>
							{/if}
							<p class="truncate text-xs tabular-nums opacity-70">
								by {mod.Author}{busy ? ` · ${statusOf(busy)}` : ""}
							</p>
						</div>
					</div>
				</td>
				<td class={!mod.Enabled ? "opacity-40" : ""}>
					<div class="flex items-center gap-1">
						{#if repo}
							<RepoVersionPicker
								mod={repo}
								installedVersion={mod.Version}
								{update}
								{patch}
								disabled={isRunning || !!busy}
								onInstall={(v) => installVersion(mod, v.version, v.sha256)}
							/>
						{:else}
							<p class="text-sm font-semibold">{mod.Version || m.common_unknown()}</p>
						{/if}
						{#if update?.installedRevoked}
							<span
								class="badge badge-error badge-xs"
								title={m.repo_installed_revoked()}>revoked</span
							>
						{/if}
					</div>
				</td>
				<td>
					<div class="flex items-center justify-end gap-1">
						{#if repo}
							<button
								class="btn btn-sm btn-square btn-ghost text-base-content/60"
								onclick={() => onOpenRepo?.(mod)}
								title={m.repo_details()}
								aria-label={m.repo_details()}
							>
								<Info size={16} />
							</button>
							<button
								class="btn btn-sm btn-square btn-ghost text-base-content/60"
								onclick={() => openUrl(repo.url)}
								title={m.repo_open_in_browser()}
								aria-label={m.repo_open_in_browser()}
							>
								<ExternalLink size={14} />
							</button>
						{/if}
						<input
							type="checkbox"
							class="toggle toggle-sm toggle-primary mx-1"
							checked={mod.Enabled}
							disabled={!!busy}
							onchange={(e) => handleToggle(mod, e.currentTarget)}
							title={mod.Enabled ? "Disable mod" : "Enable mod"}
							aria-label={mod.Enabled ? "Disable mod" : "Enable mod"}
						/>
						<button
							class="btn btn-sm btn-square btn-ghost hover:text-error"
							disabled={!!busy}
							onclick={() => onRemoveMod(mod.Name)}
							title="Delete mod"
						>
							<Trash2 size={14} />
						</button>
						<button
							class="btn btn-sm btn-square btn-ghost"
							onclick={() => showInExplorer(mod)}
							title={m.mod_show_in_explorer()}
						>
							<FolderOpen size={14} />
						</button>
					</div>
				</td>
			</tr>
		{/each}
		{#each placeholders as install (install.id)}
			{@const status = statusOf(install)}
			<tr
				class="pending-row"
				class:installing={install.progress === null}
				aria-busy="true"
				style={progressStyle(install)}
			>
				<td>
					<div class="flex items-center gap-3">
						<div
							class="bg-base-200 text-primary flex h-10 w-10 shrink-0 items-center justify-center rounded-lg"
						>
							<span class="loading loading-spinner loading-sm"></span>
						</div>
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-bold">{install.name}</p>
							<p class="text-xs tabular-nums opacity-70">
								{install.author ? `by ${install.author} · ${status}` : status}
							</p>
						</div>
					</div>
				</td>
				<td>
					<p class="text-sm font-semibold">{install.version ?? ""}</p>
				</td>
				<td>
					<div class="flex items-center justify-end gap-1">
						<div class="flex items-center gap-1 opacity-40">
							<input
								type="checkbox"
								class="toggle toggle-sm toggle-primary mx-1"
								checked
								disabled
								aria-label="Enable mod"
							/>
							<button
								class="btn btn-sm btn-square btn-ghost"
								disabled
								aria-label="Delete mod"
							>
								<Trash2 size={14} />
							</button>
							<button
								class="btn btn-sm btn-square btn-ghost"
								disabled
								aria-label={m.mod_show_in_explorer()}
							>
								<FolderOpen size={14} />
							</button>
						</div>
					</div>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

<style>
	/* Pending installs borrow the mod website's loading language: the row's divider grows into a
	   glowing primary line with the download (the site's nav progress bar), then the row carries
	   the site's shimmer sweep while the CLI installs. Both are backgrounds, so the row keeps the
	   exact footprint of a normal one. */
	.pending-row {
		--line: linear-gradient(var(--color-primary), var(--color-primary));
		--glow: linear-gradient(
			to top,
			color-mix(in oklab, var(--color-primary) 22%, transparent),
			transparent
		);
		background-image: var(--line), var(--glow);
		background-repeat: no-repeat;
		background-position:
			left bottom,
			left bottom;
		background-size:
			var(--progress) 2px,
			var(--progress) 10px;
		transition: background-size 400ms ease-out;
	}

	.pending-row.installing {
		background-image:
			var(--line), var(--glow),
			linear-gradient(
				100deg,
				transparent 30%,
				color-mix(in oklab, var(--color-base-content) 9%, transparent) 50%,
				transparent 70%
			);
		background-repeat: no-repeat, no-repeat, repeat;
		background-size:
			100% 2px,
			100% 10px,
			200% 100%;
		/* Same as the animation's first frame, or the sweep flashes mid-row before it starts. */
		background-position:
			left bottom,
			left bottom,
			150% 0;
		/* The line is already full by now; transitioning would grow the shimmer up from a strip. */
		transition: none;
		animation: row-shimmer 1.3s linear infinite;
	}

	@keyframes row-shimmer {
		from {
			background-position:
				left bottom,
				left bottom,
				150% 0;
		}
		to {
			background-position:
				left bottom,
				left bottom,
				-50% 0;
		}
	}

	/* The real row arriving in place of the placeholder, like images on the site. */
	.just-installed {
		animation: fade-in 300ms ease-out;
	}

	@keyframes fade-in {
		from {
			opacity: 0;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.pending-row.installing,
		.just-installed {
			animation: none;
		}
	}
</style>
