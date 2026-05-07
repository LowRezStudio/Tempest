<script lang="ts">
	import { Box } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import { m } from "$lib/paraglide/messages";
	import { queueItems } from "$lib/rigby/stores";
	import InstanceMenu from "./InstanceMenu.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	let isSettingUp = $derived((instance.state as { type: string }).type === "setup");
	let isDownloading = $derived((instance.state as { type: string }).type === "downloading");

	let queueItem = $derived(
		$queueItems.find((item) => item.outDir === instance.path && item.status === "running"),
	);

	let downloadProgress = $derived(queueItem?.progress?.percent ?? 0);

	function handleCardClick(e: MouseEvent) {
		const target = e.target as Element;
		if (
			target.closest("[data-bits-popover-trigger]") ||
			target.closest("[data-bits-popover-content]") ||
			target.closest("dialog")
		) {
			return;
		}
		goto(`/instance/${instance.id}`);
	}
</script>

{#if isDownloading}
	<div class="bg-base-200 rounded-lg p-4 opacity-80">
		<div class="flex items-center gap-3">
			<div
				class="w-12 h-12 rounded-lg bg-base-100 flex items-center justify-center shrink-0 overflow-hidden"
			>
				<div
					class="radial-progress text-accent"
					style="--value:{downloadProgress}; --size:3rem; --thickness:4px;"
				>
					<span class="text-xs font-semibold">{Math.round(downloadProgress)}%</span>
				</div>
			</div>
			<div class="flex-1 min-w-0">
				<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
					{#if instance.version}
						<span class="opacity-70 font-mono flex items-center gap-1.5">
							<Box size={12} />
							{instance.version}
						</span>
					{/if}
					{#if queueItem?.progress}
						<span class="text-accent">
							{m.common_downloading()}
							{Math.round(queueItem.progress.bytesPerSecond / 1024 / 1024)} MB/s
						</span>
					{:else}
						<span class="text-accent">{m.common_downloading()}</span>
					{/if}
				</div>
			</div>
			<InstanceMenu {instance} />
		</div>
	</div>
{:else}
	<div
		class="bg-base-200 hover:bg-base-300 rounded-lg transition-all duration-200 p-4 text-left cursor-pointer"
		onclick={handleCardClick}
		role="link"
		tabindex="0"
		onkeydown={(e) => {
			if (e.key === "Enter" || e.key === " ") handleCardClick(e as unknown as MouseEvent);
		}}
	>
		<div class="flex items-center gap-3">
			<div
				class="w-12 h-12 rounded-lg bg-base-100 flex items-center justify-center shrink-0 overflow-hidden"
			>
				{#if isSettingUp}
					<span class="loading loading-spinner loading-sm"></span>
				{:else}
					<Box size={24} class="opacity-60" />
				{/if}
			</div>
			<div class="flex-1 min-w-0">
				<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
					{#if instance.version}
						<span class="opacity-70 font-mono flex items-center gap-1.5">
							<Box size={12} />
							{instance.version}
						</span>
					{/if}
					{#if isSettingUp}
						<span class="text-accent">{m.common_setting_up()}</span>
					{/if}
				</div>
			</div>
			<InstanceMenu {instance} />
		</div>
	</div>
{/if}
