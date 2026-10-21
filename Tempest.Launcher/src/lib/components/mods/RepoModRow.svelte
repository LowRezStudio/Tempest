<script lang="ts">
	import { Check, Clock, Download, Gamepad2, TriangleAlert } from "@lucide/svelte";
	import { compatibleWith } from "$lib/mods/browse.svelte";
	import { cardImageOf, formatCount, kindLabel, relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { compareVersions, describeCompat } from "$lib/utils/version";
	import RepoModIcon from "./RepoModIcon.svelte";
	import RepoTrustBadge from "./RepoTrustBadge.svelte";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";
	import type { RepoMod } from "$lib/queries/repo";

	interface Props {
		mod: RepoMod;
		/** Version installed in this instance, if any */
		installedVersion?: string;
		patch?: string;
		/** This mod's install in flight */
		pending?: PendingInstall;
		isRunning: boolean;
		onInstall: (mod: RepoMod) => void;
		onOpen: (modId: string) => void;
	}

	let { mod, installedVersion, patch, pending, isRunning, onInstall, onOpen }: Props = $props();

	const image = $derived(cardImageOf(mod));
	const compatible = $derived(compatibleWith(mod, patch));
	const action = $derived.by(() => {
		if (!mod.latest) return null;
		if (installedVersion === undefined) return "install";
		return compareVersions(mod.latest.version, installedVersion) > 0 ? "update" : "installed";
	});
	const blockedReason = $derived(
		isRunning
			? m.repo_close_game_first()
			: !compatible
				? m.repo_incompatible({ patch: patch ?? "" })
				: undefined,
	);
	const compat = $derived(mod.latest ? describeCompat(mod.latest.compat) : "");

	let loaded = $state(false);
	function watchImage(img: HTMLImageElement) {
		// Already decoded from the cache: no load event comes.
		if (img.complete && img.naturalWidth > 0) loaded = true;
	}
</script>

<li
	class="hover:bg-base-300/40 @container relative flex items-center gap-4 p-3 transition-colors"
	class:pending-row={!!pending}
	class:installing={pending?.progress === null}
	aria-busy={pending ? "true" : undefined}
	style={pending
		? `--progress: ${pending.progress === null ? 100 : pending.progress * 100}%`
		: undefined}
>
	<div class="hidden w-36 shrink-0 @xl:block">
		{#if image}
			<div
				class="bg-base-300 rounded-field relative aspect-video w-full overflow-hidden"
				class:loading-shimmer={!loaded}
			>
				<img
					src={image.thumbUrl ?? image.url}
					alt=""
					loading="lazy"
					decoding="async"
					use:watchImage
					onload={() => (loaded = true)}
					onerror={() => (loaded = true)}
					class="absolute inset-0 h-full w-full object-cover transition-opacity duration-300"
					class:opacity-0={!loaded}
				/>
			</div>
		{:else}
			<div
				class="bg-base-300/60 rounded-field flex aspect-video w-full items-center justify-center"
			>
				<RepoModIcon {mod} size={40} />
			</div>
		{/if}
	</div>
	<RepoModIcon {mod} size={40} class="@xl:hidden" />

	<div class="min-w-0 flex-1">
		<div class="flex min-w-0 items-center gap-2">
			<!-- The whole row opens the mod; the stretched link keeps it one tab stop. -->
			<button
				class="cursor-pointer truncate text-left font-semibold after:absolute after:inset-0 hover:underline"
				onclick={() => onOpen(mod.id)}
			>
				{mod.name}
			</button>
			<span class="shrink-0"><RepoTrustBadge latest={mod.latest} /></span>
		</div>
		<p class="text-base-content/70 line-clamp-1 text-sm">{mod.summary}</p>
		<div
			class="text-base-content/60 mt-1 flex items-center gap-x-3 overflow-hidden text-xs whitespace-nowrap"
		>
			<span class="flex items-center gap-1.5">
				{#if mod.owner.avatar}
					<img src={mod.owner.avatar} alt="" class="size-4 rounded-full" />
				{/if}
				{mod.owner.displayName}
			</span>
			<span>{kindLabel(mod.kind)}</span>
			<span
				class="flex items-center gap-1"
				title={m.repo_downloads({ count: mod.downloads })}
			>
				<Download size={12} />
				{formatCount(mod.downloads)}
			</span>
			<span
				class="hidden items-center gap-1 @2xl:flex"
				title={new Date(mod.updatedAt).toLocaleString()}
			>
				<Clock size={12} />
				{relativeDate(mod.updatedAt)}
			</span>
			{#if mod.latest}
				{#if compatible}
					<span class="hidden items-center gap-1 @2xl:flex">
						<Gamepad2 size={12} />
						{compat === "all" ? m.repo_any_patch() : m.repo_patch({ patch: compat })}
					</span>
				{:else}
					<span
						class="text-warning flex items-center gap-1"
						title={m.repo_patch({ patch: compat })}
					>
						<TriangleAlert size={12} />
						{m.repo_incompatible({ patch: patch ?? "" })}
					</span>
				{/if}
			{/if}
		</div>
	</div>

	<!-- Above the stretched link, so these stay their own controls. -->
	<div class="relative z-10 flex shrink-0 items-center">
		{#if pending}
			<span class="btn btn-sm btn-disabled min-w-28 tabular-nums">
				<span class="loading loading-spinner loading-xs"></span>
				{pending.progress === null
					? m.mod_row_installing()
					: `${Math.round(pending.progress * 100)}%`}
			</span>
		{:else if action === "installed"}
			<span
				class="text-success flex min-w-28 items-center justify-center gap-1.5 text-sm font-semibold"
				title="v{installedVersion}"
			>
				<Check size={16} />
				{m.repo_installed()}
			</span>
		{:else if action}
			<!-- A disabled button gets no hover, so the reason sits on a wrapper. -->
			<span title={blockedReason}>
				<button
					class="btn btn-sm btn-primary min-w-28"
					disabled={!!blockedReason}
					onclick={() => onInstall(mod)}
					title={action === "update"
						? m.repo_update_available({ version: mod.latest!.version })
						: `v${mod.latest!.version}`}
				>
					<Download size={14} />
					{action === "update" ? m.repo_update() : m.repo_install()}
				</button>
			</span>
		{/if}
	</div>
</li>
