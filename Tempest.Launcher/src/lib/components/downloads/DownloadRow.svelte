<script lang="ts">
	import {
		Box,
		Clock,
		Files,
		FolderOpen,
		Gauge,
		HardDrive,
		Play,
		RotateCcw,
		Wrench,
		X,
	} from "@lucide/svelte";
	import PatchBanner from "$lib/components/library/PatchBanner.svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { relativeDate } from "$lib/mods/listing";
	import { removeModDownload } from "$lib/mods/modDownloads.svelte";
	import { m } from "$lib/paraglide/messages";
	import { folderName, formatBytes, formatSpeed } from "$lib/rigby/format";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { patchSummary } from "$lib/utils/versions";
	import type { QueueItem } from "$lib/rigby/stores.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		item: QueueItem;
		/** The instance it downloads into, unless it was removed from the library */
		instance?: Instance;
		/** A mod's icon, shown instead of the patch's art */
		icon?: string | null;
		oncancel: (item: QueueItem) => void;
	}

	let { item, instance, icon, oncancel }: Props = $props();

	const color = $derived(getInstanceColor(instance));
	const contrast = $derived(getContrastColor(color));
	const settingUp = $derived(item.status === "complete" && instance?.state.type === "setup");
	const href = $derived(
		item.status === "complete" && instance?.state.type === "prepared"
			? `/instance/${instance.id}`
			: undefined,
	);

	const badge = $derived.by(() => {
		if (item.mod) return { label: m.downloads_kind_mod(), tone: "badge-ghost" };
		if (item.status === "pending") {
			return { label: m.library_badge_queued(), tone: "badge-ghost" };
		}
		if (item.status === "paused") return { label: m.common_paused(), tone: "badge-warning" };
		if (item.status === "error") {
			return { label: m.downloads_badge_failed(), tone: "badge-error" };
		}
		if (settingUp) return { label: m.library_badge_setup(), tone: "badge-primary" };
		return null;
	});
</script>

<!-- No `?` on snippet parameters: Svelte leaves it in the compiled JS and the module fails to load -->
{#snippet info(text: string, Icon: typeof Clock | null = null, tone = "", title = "")}
	<span
		class={[
			"flex min-w-0 items-center gap-1 text-xs whitespace-nowrap tabular-nums",
			tone || "text-base-content/60",
		]}
		title={title || text}
	>
		{#if Icon}
			<Icon size={12} class="shrink-0" />
		{/if}
		<span class="truncate">{text}</span>
	</span>
{/snippet}

<li
	class="hover:bg-base-300/40 @container relative flex items-center gap-4 p-3 transition-colors"
	class:pending-row={settingUp}
	class:installing={settingUp}
	aria-busy={settingUp ? "true" : undefined}
>
	{#if item.mod}
		<!-- As wide as the patches' banners, so the rows' text lines up -->
		<div class="flex w-10 shrink-0 justify-center @xl:w-28">
			<div
				class="bg-base-300 flex size-10 items-center justify-center overflow-hidden rounded-lg @xl:size-14"
			>
				{#if icon}
					<img src={icon} alt="" draggable="false" class="size-full object-cover" />
				{:else}
					<Box size={20} class="opacity-60" />
				{/if}
			</div>
		</div>
	{:else}
		<PatchBanner version={instance?.version} {color} class="hidden w-28 shrink-0 @xl:block" />
		<div
			class="rounded-field flex size-10 shrink-0 items-center justify-center text-[11px] font-bold tabular-nums @xl:hidden"
			style="background-color: {color}; color: {contrast};"
		>
			{#if instance?.version}
				{instance.version}
			{:else}
				<PaladinsIcon size={28} color={contrast} />
			{/if}
		</div>
	{/if}

	<!-- One grid, so each line of stats sits on the subtitle line beside it. Every slot renders
	     (empty when there is nothing to say) to keep the cells in their columns. -->
	<div
		class="grid min-w-0 flex-1 grid-cols-2 items-center gap-x-3 gap-y-0.5 @2xl:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_minmax(0,1fr)] @2xl:gap-x-4"
	>
		<div class="col-span-full flex min-w-0 items-center gap-2">
			{#if href}
				<!-- The whole row opens the instance; the stretched link keeps it one tab stop. -->
				<a
					{href}
					class="truncate font-semibold after:absolute after:inset-0 hover:underline"
				>
					{item.mod?.name ?? instance?.label}
				</a>
			{:else}
				<span class="truncate font-semibold">
					{item.mod?.name ?? instance?.label ?? folderName(item.outDir)}
				</span>
			{/if}
			{#if badge}
				<span class="badge badge-sm shrink-0 whitespace-nowrap {badge.tone}"
					>{badge.label}</span
				>
			{/if}
		</div>

		<p class="text-base-content/70 col-span-full line-clamp-1 text-sm @2xl:col-span-1">
			{#if item.mod}
				{m.downloads_mod_for({
					version: item.mod.version,
					patch: item.mod.patchLabel ?? folderName(item.outDir),
				})}
			{:else}
				{instance ? patchSummary(instance) : item.outDir}
			{/if}
		</p>
		{#if item.status === "pending"}
			{@render info(m.common_waiting_in_queue())}
			<span></span>
		{:else if item.status === "error"}
			{@render info(item.error ?? "", null, "col-span-2 text-error")}
		{:else if item.mod?.install}
			{@const percent = item.mod.install.percent}
			{@render info(
				percent === null
					? m.mod_row_installing()
					: m.mod_row_patching({ percent: Math.round(percent) }),
				null,
				"col-span-2 text-primary",
			)}
		{:else if item.status === "running"}
			{@render info(`${Math.round(item.progress?.percent ?? 0)}%`)}
			{@render info(formatSpeed(item.progress?.bytesPerSecond ?? 0), Gauge)}
		{:else if item.status === "complete" && item.result}
			{#if item.mod}
				<span></span>
			{:else}
				{@render info(m.downloads_file_count({ count: item.result.files }), Files)}
			{/if}
			{@render info(
				m.downloads_written_size({ size: formatBytes(item.result.diskWriteBytes) }),
				HardDrive,
			)}
		{:else}
			<span class="col-span-2"></span>
		{/if}

		<div class="hidden min-w-0 @2xl:block">
			{#if instance}
				{@render info(item.outDir, FolderOpen)}
			{/if}
		</div>
		{#if item.status === "complete" && item.result}
			{#if item.finishedAt}
				{@render info(
					relativeDate(new Date(item.finishedAt).toISOString()),
					Clock,
					"",
					new Date(item.finishedAt).toLocaleString(),
				)}
			{:else}
				<span></span>
			{/if}
			<!-- A download writes every file, so only a check has repairs worth pointing out -->
			{#if item.repair && item.result.repairedFiles > 0}
				{@render info(
					m.downloads_repaired_count({ count: item.result.repairedFiles }),
					Wrench,
					"text-warning",
				)}
			{/if}
		{/if}
	</div>

	<!-- Above the stretched link, so these stay their own controls. -->
	<div class="relative z-10 flex shrink-0 items-center gap-1">
		{#if item.status === "paused"}
			<button class="btn btn-sm min-w-24" onclick={() => restoreQueue.resume(item.id)}>
				<Play size={14} />
				{m.library_resume()}
			</button>
		{:else if item.status === "error"}
			<button class="btn btn-sm min-w-24" onclick={() => restoreQueue.retry(item.id)}>
				<RotateCcw size={14} />
				{m.common_retry()}
			</button>
		{/if}
		{#if item.status === "complete"}
			<button
				class="btn btn-ghost btn-sm btn-square"
				title={m.common_dismiss()}
				aria-label={m.common_dismiss()}
				onclick={() =>
					item.mod ? removeModDownload(item.id) : restoreQueue.remove(item.id)}
			>
				<X size={16} />
			</button>
		{:else if !item.mod}
			<button
				class="btn btn-ghost btn-sm btn-square hover:text-error"
				title={m.downloads_cancel()}
				aria-label={m.downloads_cancel()}
				onclick={() => oncancel(item)}
			>
				<X size={16} />
			</button>
		{/if}
	</div>
</li>
