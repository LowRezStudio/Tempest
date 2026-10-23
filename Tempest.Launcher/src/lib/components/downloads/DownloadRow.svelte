<script lang="ts">
	import {
		Clock,
		CloudDownload,
		Files,
		FolderOpen,
		HardDrive,
		Play,
		RotateCcw,
		ShieldCheck,
		Wrench,
		X,
	} from "@lucide/svelte";
	import PatchBanner from "$lib/components/library/PatchBanner.svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { folderName, formatBytes } from "$lib/rigby/format";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { patchSummary } from "$lib/utils/versions";
	import type { QueueItem } from "$lib/rigby/stores.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		item: QueueItem;
		/** The instance it downloads into, unless it was removed from the library */
		instance?: Instance;
		oncancel: (item: QueueItem) => void;
	}

	let { item, instance, oncancel }: Props = $props();

	const color = $derived(getInstanceColor(instance));
	const contrast = $derived(getContrastColor(color));
	const settingUp = $derived(item.status === "complete" && instance?.state.type === "setup");
	const href = $derived(
		item.status === "complete" && instance?.state.type === "prepared"
			? `/instance/${instance.id}`
			: undefined,
	);

	const badge = $derived.by(() => {
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

<li
	class="hover:bg-base-300/40 @container relative flex items-center gap-4 p-3 transition-colors"
	class:pending-row={settingUp}
	class:installing={settingUp}
	aria-busy={settingUp ? "true" : undefined}
>
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

	<div class="min-w-0 flex-1">
		<div class="flex min-w-0 items-center gap-2">
			{#if href}
				<!-- The whole row opens the instance; the stretched link keeps it one tab stop. -->
				<a
					{href}
					class="truncate font-semibold after:absolute after:inset-0 hover:underline"
				>
					{instance?.label}
				</a>
			{:else}
				<span class="truncate font-semibold">
					{instance?.label ?? folderName(item.outDir)}
				</span>
			{/if}
			{#if badge}
				<span class="badge badge-sm shrink-0 whitespace-nowrap {badge.tone}"
					>{badge.label}</span
				>
			{/if}
		</div>
		<p class="text-base-content/70 line-clamp-1 text-sm">
			{instance ? patchSummary(instance) : item.outDir}
		</p>
		<div
			class="text-base-content/60 mt-1 flex items-center gap-x-3 overflow-hidden text-xs whitespace-nowrap"
		>
			<span class="flex items-center gap-1">
				{#if item.repair}
					<ShieldCheck size={12} />
					{m.downloads_kind_verify()}
				{:else}
					<CloudDownload size={12} />
					{m.downloads_kind_download()}
				{/if}
			</span>
			{#if item.status === "pending"}
				<span>{m.common_waiting_in_queue()}</span>
			{:else if item.status === "paused"}
				<span>{m.downloads_resumes()}</span>
			{:else if item.status === "error"}
				<span class="text-error min-w-0 truncate" title={item.error}>{item.error}</span>
			{:else if item.status === "complete" && item.result}
				<span class="flex items-center gap-1 tabular-nums">
					<Files size={12} />
					{m.downloads_file_count({ count: item.result.files })}
				</span>
				<span class="flex items-center gap-1 tabular-nums">
					<HardDrive size={12} />
					{m.downloads_written_size({ size: formatBytes(item.result.diskWriteBytes) })}
				</span>
				{#if item.result.repairedFiles > 0}
					<span class="text-warning flex items-center gap-1 tabular-nums">
						<Wrench size={12} />
						{m.downloads_repaired_count({ count: item.result.repairedFiles })}
					</span>
				{/if}
				{#if item.finishedAt}
					<span
						class="flex items-center gap-1"
						title={new Date(item.finishedAt).toLocaleString()}
					>
						<Clock size={12} />
						{relativeDate(new Date(item.finishedAt).toISOString())}
					</span>
				{/if}
			{/if}
			<span class="hidden min-w-0 items-center gap-1 @2xl:flex" title={item.outDir}>
				<FolderOpen size={12} class="shrink-0" />
				<span class="truncate">{item.outDir}</span>
			</span>
		</div>
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
		{:else if href}
			<a {href} class="btn btn-sm min-w-24">{m.downloads_open()}</a>
		{/if}
		{#if item.status === "complete"}
			<button
				class="btn btn-ghost btn-sm btn-square"
				title={m.common_dismiss()}
				aria-label={m.common_dismiss()}
				onclick={() => restoreQueue.remove(item.id)}
			>
				<X size={16} />
			</button>
		{:else}
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
