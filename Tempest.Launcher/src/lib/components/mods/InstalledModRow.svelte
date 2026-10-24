<script lang="ts">
	import {
		ArrowRight,
		BadgeCheck,
		Box,
		ChevronDown,
		ExternalLink,
		FolderOpen,
		Info,
		PackageX,
		Power,
		ShieldX,
		Trash2,
		TriangleAlert,
	} from "@lucide/svelte";
	import { trustOf } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import type { ModRecord } from "$lib/core/mods";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";
	import type { RepoMod, RepoUpdate } from "$lib/queries/repo";
	import type { Dependency } from "$lib/utils/deps";

	interface Props {
		mod: ModRecord;
		repo?: RepoMod;
		/** Undefined while the repository is still being asked, null when there is none. */
		icon: string | null | undefined;
		update?: RepoUpdate;
		busy?: PendingInstall;
		missing?: Dependency[];
		/** All of `missing` is installed, only turned off */
		missingTurnedOff: boolean;
		isRunning: boolean;
		fresh: boolean;
		onToggle: (mod: ModRecord, input: HTMLInputElement) => void;
		/** Its repository page, or its asset files when it has none */
		onOpen: (mod: ModRecord) => void;
		/** The repository page's version history, for switching versions */
		onOpenVersions: (mod: ModRecord) => void;
		onShowInExplorer: (mod: ModRecord) => void;
		onRename: (mod: ModRecord, name: string) => void;
		onRemove: (mod: ModRecord) => void;
		onFetchMissing: (mod: ModRecord) => void;
	}

	let {
		mod,
		repo,
		icon,
		update,
		busy,
		missing,
		missingTurnedOff,
		isRunning,
		fresh,
		onToggle,
		onOpen,
		onOpenVersions,
		onShowInExplorer,
		onRename,
		onRemove,
		onFetchMissing,
	}: Props = $props();

	const trust = $derived(repo?.latest?.signer?.trust);
	const updateTo = $derived(update?.updateAvailable ? update.latest?.version : undefined);
	const status = $derived(
		busy &&
			(busy.progress === null
				? m.mod_row_installing()
				: `${Math.round(busy.progress * 100)}%`),
	);
	const toggleLabel = $derived(
		mod.Enabled ? m.mods_disable({ name: mod.Name }) : m.mods_enable({ name: mod.Name }),
	);

	let renaming = $state(false);
	let editName = $state("");

	function startRename() {
		editName = mod.Name;
		renaming = true;
	}

	function commitRename() {
		if (!renaming) return;
		// Cleared first: removing the input fires blur, which would commit again.
		renaming = false;
		const name = editName.trim();
		if (name && name !== mod.Name) onRename(mod, name);
	}

	function onRenameKey(e: KeyboardEvent) {
		if (e.key === "Enter") {
			e.preventDefault();
			commitRename();
		} else if (e.key === "Escape") {
			e.preventDefault();
			renaming = false;
		}
	}

	function focusAndSelect(node: HTMLInputElement) {
		node.focus();
		node.select();
	}
</script>

<tr
	class="hover:bg-base-300/40"
	class:just-installed={fresh}
	class:pending-row={!!busy}
	class:installing={busy?.progress === null}
	aria-busy={busy ? "true" : undefined}
	style:--progress={busy ? `${busy.progress === null ? 100 : busy.progress * 100}%` : undefined}
>
	<td class={["transition-opacity", !mod.Enabled && "opacity-40"]}>
		<div class="flex items-center gap-3">
			<button
				class="bg-base-300 text-base-content/60 flex size-10 shrink-0 cursor-pointer items-center justify-center overflow-hidden rounded-lg transition-transform hover:scale-105 active:scale-95"
				onclick={() => onOpen(mod)}
				title={repo ? m.repo_details() : m.mod_updated_files()}
				aria-label="{repo ? m.repo_details() : m.mod_updated_files()} {mod.Name}"
			>
				{#if busy}
					<span class="loading loading-spinner loading-sm"></span>
				{:else if icon}
					<img
						src={icon}
						alt=""
						draggable="false"
						class="size-full object-cover"
						class:grayscale={!mod.Enabled}
					/>
				{:else if icon === undefined}
					<div class="loading-shimmer size-full"></div>
				{:else if mod.Enabled}
					<Box size={20} />
				{:else}
					<PackageX size={20} />
				{/if}
			</button>
			<div class="min-w-0 flex-1">
				{#if renaming}
					<input
						type="text"
						class="input input-xs -mx-1.5 w-full max-w-xs px-1.5 text-sm font-bold"
						aria-label={m.mod_rename_dialog_label()}
						bind:value={editName}
						use:focusAndSelect
						onkeydown={onRenameKey}
						onblur={commitRename}
					/>
				{:else}
					<div class="flex min-w-0 items-center gap-1">
						<button
							class="hover:bg-base-content/10 -ml-1.5 block max-w-full cursor-text truncate rounded pr-1 pl-1.5 text-left text-sm font-bold transition-colors"
							onclick={startRename}
							title={m.mod_rename()}
						>
							{mod.Name}
						</button>
						{#if trust === "first-party" || trust === "trusted"}
							<span class="flex shrink-0" title={trustOf(repo?.latest ?? null).hint}>
								<BadgeCheck size={15} class="fill-primary text-base-200" />
							</span>
						{/if}
					</div>
				{/if}
				<p class="truncate text-xs tabular-nums opacity-70">
					by {mod.Author}{status ? ` · ${status}` : ""}
				</p>
			</div>
		</div>
	</td>
	<td class={["transition-opacity", !mod.Enabled && "opacity-40"]}>
		<div class="flex min-w-0 items-center gap-1">
			{#if repo}
				<button
					class={[
						"btn btn-ghost btn-xs -ml-2 min-w-0 gap-1 text-sm font-semibold",
						updateTo && "text-primary",
					]}
					title={updateTo
						? m.repo_update_available({ version: updateTo })
						: m.repo_versions()}
					onclick={() => onOpenVersions(mod)}
				>
					<span class="truncate">{mod.Version || m.common_unknown()}</span>
					{#if updateTo}
						<ArrowRight size={12} class="shrink-0 opacity-70" />
						<span class="truncate">{updateTo}</span>
					{/if}
					<ChevronDown size={12} class="shrink-0 opacity-50" />
				</button>
			{:else}
				<p class="truncate text-sm font-semibold" title={mod.Version}>
					{mod.Version || m.common_unknown()}
				</p>
			{/if}
			{#if update?.installedRevoked}
				<span
					class="badge badge-error badge-soft badge-sm shrink-0 gap-1"
					title={m.repo_installed_revoked()}
				>
					<ShieldX size={12} />
					{m.repo_trust_revoked()}
				</span>
			{/if}
			{#if missing && mod.Enabled}
				{@const names = missing.map((d) => d.id).join(", ")}
				<button
					class="badge badge-warning badge-soft badge-sm min-w-0 cursor-pointer gap-1"
					title={missingTurnedOff
						? m.mod_dependencies_off_hint()
						: m.mod_missing_dependencies_hint()}
					disabled={isRunning || !!busy}
					onclick={() => onFetchMissing(mod)}
				>
					{#if missingTurnedOff}
						<Power size={12} class="shrink-0" />
					{:else}
						<TriangleAlert size={12} class="shrink-0" />
					{/if}
					<span class="truncate">
						{missingTurnedOff
							? m.mod_dependencies_off({ names })
							: m.mod_missing_dependencies({ names })}
					</span>
				</button>
			{/if}
		</div>
	</td>
	<td>
		<div class="flex items-center justify-end gap-1">
			{#if repo}
				<button
					class="btn btn-sm btn-square btn-ghost text-base-content/60"
					onclick={() => onOpen(mod)}
					title={m.repo_details()}
					aria-label="{m.repo_details()} {mod.Name}"
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
				onchange={(e) => onToggle(mod, e.currentTarget)}
				title={toggleLabel}
				aria-label={toggleLabel}
			/>
			<button
				class="btn btn-sm btn-square btn-ghost hover:text-error"
				disabled={!!busy}
				onclick={() => onRemove(mod)}
				title={m.common_remove()}
				aria-label="{m.common_remove()} {mod.Name}"
			>
				<Trash2 size={14} />
			</button>
			<button
				class="btn btn-sm btn-square btn-ghost"
				onclick={() => onShowInExplorer(mod)}
				title={m.mod_show_in_explorer()}
				aria-label={m.mod_show_in_explorer()}
			>
				<FolderOpen size={14} />
			</button>
		</div>
	</td>
</tr>
