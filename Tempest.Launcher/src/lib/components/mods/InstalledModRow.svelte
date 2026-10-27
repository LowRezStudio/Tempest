<script lang="ts">
	import {
		ArrowRight,
		BadgeCheck,
		Box,
		FileText,
		Layers,
		PackageX,
		Power,
		ShieldX,
		Trash2,
		TriangleAlert,
	} from "@lucide/svelte";
	import { trustOf } from "$lib/mods/listing";
	import { pendingStatus } from "$lib/mods/pendingInstalls.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { ModRecord } from "$lib/core/mods";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";
	import type { RepoMod, RepoUpdate } from "$lib/queries/repo";
	import type { Dependency } from "$lib/utils/deps";

	interface Props {
		mod: ModRecord;
		config?: string;
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
		onToggle: (mod: ModRecord) => void;
		/** Its repository page, or its asset files when it has none */
		onOpen: (mod: ModRecord) => void;
		/** The repository page's version history, for switching versions */
		onOpenVersions: (mod: ModRecord) => void;
		onViewConfig: (mod: ModRecord, config: string) => void;
		onRemove: (mod: ModRecord) => void;
		onFetchMissing: (mod: ModRecord) => void;
	}

	let {
		mod,
		config,
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
		onViewConfig,
		onRemove,
		onFetchMissing,
	}: Props = $props();

	const trust = $derived(repo?.latest?.signer?.trust);
	const updateTo = $derived(update?.updateAvailable ? update.latest?.version : undefined);
	const status = $derived(busy && pendingStatus(busy));
	const toggleLabel = $derived(
		mod.Enabled ? m.mods_disable({ name: mod.Name }) : m.mods_enable({ name: mod.Name }),
	);

	// Not a stretched button: the faded cells of a turned-off mod are stacking contexts that would
	// sit over it.
	function openFromRow(e: MouseEvent) {
		if (e.target instanceof Element && e.target.closest("button, a, input, label")) return;
		onOpen(mod);
	}
</script>

<!-- A click anywhere but on a control opens the mod; from the keyboard, its name does. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<tr
	class="hover:bg-base-300/40 cursor-pointer"
	onclick={openFromRow}
	class:just-installed={fresh}
	class:pending-row={!!busy}
	class:installing={busy?.progress === null}
	aria-busy={busy ? "true" : undefined}
	style:--progress={busy ? `${busy.progress === null ? 100 : busy.progress * 100}%` : undefined}
>
	<td class={["transition-opacity", !mod.Enabled && "opacity-40"]}>
		<div class="flex items-center gap-3">
			<div
				class="bg-base-300 text-base-content/60 flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg"
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
			</div>
			<div class="min-w-0 flex-1">
				<div class="flex min-w-0 items-center gap-1">
					<button
						class="block max-w-full cursor-pointer truncate text-left text-sm font-bold hover:underline"
						onclick={() => onOpen(mod)}
					>
						{mod.Name}
					</button>
					{#if trust === "first-party" || trust === "trusted"}
						<span class="flex shrink-0" title={trustOf(repo?.latest ?? null).hint}>
							<BadgeCheck size={15} class="fill-primary text-base-200" />
						</span>
					{/if}
				</div>
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
	<td class="text-center">
		<input
			type="checkbox"
			class="toggle toggle-primary"
			checked={mod.Enabled}
			disabled={!!busy}
			onchange={() => onToggle(mod)}
			title={toggleLabel}
			aria-label={toggleLabel}
		/>
	</td>
	<td>
		<div class="flex items-center justify-end gap-1">
			{#if config !== undefined}
				<button
					class="btn btn-sm btn-square btn-ghost"
					onclick={() => onViewConfig(mod, config)}
					title="View mod config"
					aria-label="View config for {mod.Name}"
				>
					<FileText size={14} />
				</button>
			{/if}
			<button
				class="btn btn-sm btn-square btn-ghost hover:text-error"
				disabled={!!busy}
				onclick={() => onRemove(mod)}
				title={m.common_remove()}
				aria-label="{m.common_remove()} {mod.Name}"
			>
				<Trash2 size={14} />
			</button>
		</div>
	</td>
</tr>
