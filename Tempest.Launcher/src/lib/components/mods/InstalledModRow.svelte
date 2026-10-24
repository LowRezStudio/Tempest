<script lang="ts">
	import {
		ArrowUp,
		BadgeCheck,
		CodeXml,
		Cpu,
		EllipsisVertical,
		ExternalLink,
		FileBox,
		FolderOpen,
		HardDrive,
		History,
		Info,
		Layers,
		ListTree,
		Pencil,
		Power,
		ShieldX,
		Trash2,
		TriangleAlert,
	} from "@lucide/svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { kindLabel, modIconFallback, trustOf } from "$lib/mods/listing";
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
		/** Approved versions on the repository */
		versionCount: number;
		isRunning: boolean;
		fresh: boolean;
		onToggle: (mod: ModRecord, input: HTMLInputElement) => void;
		onOpen: (mod: ModRecord) => void;
		/** The repository page's version history, for switching versions */
		onOpenVersions: (mod: ModRecord) => void;
		onOpenFiles: (mod: ModRecord) => void;
		onShowInExplorer: (mod: ModRecord) => void;
		onRename: (mod: ModRecord, name: string) => void;
		onRemove: (mod: ModRecord) => void;
		onUpdate: (mod: ModRecord) => void;
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
		versionCount,
		isRunning,
		fresh,
		onToggle,
		onOpen,
		onOpenVersions,
		onOpenFiles,
		onShowInExplorer,
		onRename,
		onRemove,
		onUpdate,
		onFetchMissing,
	}: Props = $props();

	const kindIcon = { assets: FileBox, scripts: CodeXml, native: Cpu } as const;
	const KindIcon = $derived(repo ? kindIcon[repo.kind] : HardDrive);
	const trust = $derived(repo?.latest?.signer?.trust);
	const updateTo = $derived(update?.updateAvailable ? update.latest?.version : undefined);
	const initials = $derived(modIconFallback(mod.Id, mod.Name).initials);
	const status = $derived(
		busy &&
			(busy.progress === null
				? m.mod_row_installing()
				: `${Math.round(busy.progress * 100)}%`),
	);

	let menuOpen = $state(false);
	const pick = (action: () => void) => () => {
		menuOpen = false;
		action();
	};

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

<li class="relative" class:just-installed={fresh} aria-busy={busy ? "true" : undefined}>
	{#if busy}
		<div
			class="pending-row pointer-events-none absolute inset-0"
			class:installing={busy.progress === null}
			style:--progress="{busy.progress === null ? 100 : busy.progress * 100}%"
		></div>
	{/if}

	<!-- Relative so the stretched name button covers this line only, not the versions below -->
	<div
		class="hover:bg-base-300/40 relative flex items-center gap-3 px-3 py-2.5 transition-colors"
	>
		<div
			class={[
				"bg-base-300 text-base-content/50 relative flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg text-sm font-bold transition-[filter,opacity] duration-300",
				!mod.Enabled && "opacity-50 grayscale",
			]}
		>
			{#if icon}
				<img src={icon} alt="" draggable="false" class="size-full object-cover" />
			{:else if icon === null}
				{initials}
			{:else}
				<div class="loading-shimmer size-full"></div>
			{/if}
			{#if busy}
				<div class="bg-base-300/80 absolute inset-0 flex items-center justify-center">
					<span class="loading loading-spinner loading-sm opacity-60"></span>
				</div>
			{/if}
		</div>

		<div class="min-w-0 flex-1">
			{#if renaming}
				<input
					type="text"
					class="input input-xs relative z-10 -mx-1.5 w-full max-w-xs px-1.5 text-sm font-semibold"
					aria-label={m.mod_rename_dialog_label()}
					bind:value={editName}
					use:focusAndSelect
					onkeydown={onRenameKey}
					onblur={commitRename}
				/>
			{:else}
				<div class="flex min-w-0 items-baseline gap-1.5">
					<!-- The whole row opens the mod; the stretched button keeps it one tab stop. -->
					<button
						class={[
							"focus-visible:after:ring-primary/70 min-w-0 shrink cursor-pointer truncate text-left text-sm font-semibold outline-none after:absolute after:inset-0 hover:underline focus-visible:after:ring-2 focus-visible:after:ring-inset",
							!mod.Enabled && "text-base-content/50",
						]}
						onclick={() => onOpen(mod)}
					>
						{mod.Name}
					</button>
					{#if trust === "first-party" || trust === "trusted"}
						<span
							class="flex shrink-0 self-center"
							title={trustOf(repo?.latest ?? null).hint}
						>
							<BadgeCheck size={15} class="fill-primary text-base-200" />
						</span>
					{/if}
					<span
						class={[
							"min-w-16 flex-1 truncate text-xs",
							mod.Enabled ? "text-base-content/50" : "text-base-content/35",
						]}
					>
						by {mod.Author}
					</span>
				</div>
			{/if}
			<div
				class={[
					"mt-0.5 flex min-w-0 items-center gap-1.5 text-xs whitespace-nowrap",
					mod.Enabled ? "text-base-content/60" : "text-base-content/35",
				]}
			>
				{#if status}
					<span class="shrink-0 tabular-nums">{status}</span>
					<span aria-hidden="true">·</span>
				{/if}
				<span
					class="flex shrink-0 items-center gap-1"
					title={repo ? undefined : m.mods_local_hint()}
				>
					<KindIcon size={12} />
					{repo ? kindLabel(repo.kind) : m.mods_local()}
				</span>
				{#if repo?.summary && repo.summary.toLowerCase() !== mod.Name.toLowerCase()}
					<span aria-hidden="true">·</span>
					<span class="truncate">{repo.summary}</span>
				{/if}
			</div>
		</div>

		<!-- Above the stretched button, so these stay their own controls. -->
		<div class="relative z-10 flex shrink-0 items-center gap-2">
			{#if updateTo}
				<button
					class="btn btn-soft btn-primary btn-xs gap-1 font-mono"
					disabled={isRunning || !!busy}
					onclick={() => onUpdate(mod)}
					aria-label={m.repo_update_available({ version: updateTo })}
				>
					<ArrowUp size={12} />
					{updateTo}
				</button>
			{/if}
			{#if update?.installedRevoked}
				<span
					class="badge badge-error badge-soft badge-sm gap-1"
					title={m.repo_installed_revoked()}
				>
					<ShieldX size={12} />
					{m.repo_trust_revoked()}
				</span>
			{/if}
			{#if missing && mod.Enabled}
				{@const names = missing.map((d) => d.id).join(", ")}
				<button
					class="badge badge-warning badge-soft badge-sm max-w-56 cursor-pointer gap-1"
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

			<!-- A fixed-width column with the version centered, so every row's sits on one axis -->
			<div
				class={[
					"flex w-36 justify-center font-mono text-xs",
					mod.Enabled ? "text-base-content/70" : "text-base-content/35",
				]}
			>
				{#if repo}
					<button
						class="rounded-field hover:bg-base-content/10 flex max-w-full min-w-0 cursor-pointer items-center gap-1.5 px-1.5 py-1 transition-colors"
						title={m.repo_versions()}
						onclick={() => onOpenVersions(mod)}
					>
						<span class="truncate">{mod.Version || m.common_unknown()}</span>
						{#if versionCount > 1}
							<span
								class="bg-base-content/10 flex shrink-0 items-center gap-0.5 rounded px-1 text-[11px] leading-4"
								aria-label={m.mods_version_count({ count: versionCount })}
							>
								<Layers size={11} />
								{versionCount}
							</span>
						{/if}
					</button>
				{:else}
					<span class="truncate px-1.5 py-1" title={mod.Version}>
						{mod.Version || m.common_unknown()}
					</span>
				{/if}
			</div>
		</div>

		<div class="relative z-10 ml-1 flex shrink-0 items-center">
			<PopoverMenu
				triggerClass="btn btn-ghost btn-sm btn-square text-base-content/50 hover:text-base-content"
				triggerLabel={m.mods_more_actions()}
				menuClass="w-56"
				bind:open={menuOpen}
			>
				{#snippet trigger()}
					<EllipsisVertical size={16} />
				{/snippet}
				{#snippet children()}
					{#if repo}
						<PopoverMenuItem onclick={pick(() => onOpen(mod))}>
							<Info size={16} />
							{m.repo_details()}
						</PopoverMenuItem>
						<PopoverMenuItem onclick={pick(() => onOpenVersions(mod))}>
							<History size={16} />
							{m.repo_versions()}
						</PopoverMenuItem>
						<PopoverMenuItem onclick={pick(() => openUrl(repo.url))}>
							<ExternalLink size={16} />
							{m.repo_open_in_browser()}
						</PopoverMenuItem>
					{/if}
					<PopoverMenuItem onclick={pick(() => onOpenFiles(mod))}>
						<ListTree size={16} />
						{m.mod_updated_files()}
					</PopoverMenuItem>
					<PopoverMenuItem onclick={pick(() => onShowInExplorer(mod))}>
						<FolderOpen size={16} />
						{m.mod_show_in_explorer()}
					</PopoverMenuItem>
					<PopoverMenuItem onclick={pick(startRename)} disabled={!!busy}>
						<Pencil size={16} />
						{m.mod_rename()}
					</PopoverMenuItem>
				{/snippet}
			</PopoverMenu>
			<button
				class="btn btn-ghost btn-sm btn-square text-base-content/50 hover:text-error"
				disabled={!!busy}
				onclick={() => onRemove(mod)}
				title={m.common_remove()}
				aria-label="{m.common_remove()} {mod.Name}"
			>
				<Trash2 size={15} />
			</button>
			<input
				type="checkbox"
				class="toggle toggle-primary ml-2"
				checked={mod.Enabled}
				disabled={!!busy}
				onchange={(e) => onToggle(mod, e.currentTarget)}
				title={mod.Enabled
					? m.mods_disable({ name: mod.Name })
					: m.mods_enable({ name: mod.Name })}
				aria-label={mod.Enabled
					? m.mods_disable({ name: mod.Name })
					: m.mods_enable({ name: mod.Name })}
			/>
		</div>
	</div>
</li>
