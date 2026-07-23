<script lang="ts">
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { Gamepad2, Pause, Play, Square, Trash2 } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import { m } from "$lib/paraglide/messages";
	import { queueItems } from "$lib/rigby/stores.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import { deleteInstance } from "$lib/core/instance-delete";
	import { processesList } from "$lib/stores/processes.svelte";
	import { createLaunchGameMutation, createKillGameMutation } from "$lib/queries/core";
	import InstanceMenu from "./InstanceMenu.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	let isSettingUp = $derived(instance.state.type === "setup");
	let isDownloading = $derived(instance.state.type === "downloading");
	let isPaused = $derived(instance.state.type === "paused");
	let isActive = $derived(isDownloading || isPaused);
	let iconBg = $derived(getInstanceColor(instance));

	let queueItem = $derived(
		queueItems.value.find(
			(item) => item.outDir === instance.path && (item.status === "running" || item.status === "pending"),
		),
	);

	let downloadProgress = $derived(queueItem?.progress?.percent ?? 0);

	const launchMutation = createLaunchGameMutation();
	const killMutation = createKillGameMutation();
	let isLaunching = $derived(launchMutation.isPending);
	let isKilling = $derived(killMutation.isPending);
	let isBusy = $derived(isLaunching || isKilling);

	let isRunning = $derived(processesList.value.some((p) => p.instance.id === instance.id));

	let showDeleteConfirm = $state(false);

	async function handleDeleteConfirm(deleteData: boolean) {
		if (!instance) return;
		await deleteInstance(instance, deleteData);
	}

	function handleCardClick(e: MouseEvent) {
		const target = e.target as Element;
		if (
			target.closest("[data-bits-popover-trigger]") ||
			target.closest("[data-bits-popover-content]") ||
			target.closest("dialog") ||
			target.closest(".delete-instance-btn")
		) {
			return;
		}
		goto(`/instance/${instance.id}`);
	}
</script>

{#if isActive}
	<div class="bg-base-200 rounded-lg p-4 opacity-80">
		<div class="flex items-center gap-3">
			<div
				class="w-12 h-12 rounded-lg flex items-center justify-center shrink-0 overflow-hidden"
				style="background-color: {iconBg}"
			>
				{#if queueItem?.status === "pending" || !isDownloading}
					<Pause
						size={24}
						style="color: {getContrastColor(getInstanceColor(instance))};"
					/>
				{:else}
					<div
						class="radial-progress"
						style="--value:{downloadProgress}; --size:3rem; --thickness:4px; color: {getContrastColor(
							getInstanceColor(instance),
						)};"
					>
						<span
							class="text-xs font-semibold"
							style="color: {getContrastColor(getInstanceColor(instance))};"
							>{Math.round(downloadProgress)}%</span
						>
					</div>
				{/if}
			</div>
			<div class="flex-1 min-w-0">
				<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
					{#if instance.version && !isDownloading}
						<span class="opacity-70 font-mono flex items-center gap-1.5">
							<Gamepad2 size={12} />
							{instance.version}
						</span>
					{/if}
					{#if isDownloading}
						{#if queueItem?.status === "pending"}
							<span class="text-accent">{m.common_waiting_in_queue()}</span>
						{:else if queueItem?.progress}
							<span class="text-accent">
								{m.common_downloading()}
								{Math.round(queueItem.progress.bytesPerSecond / 1024 / 1024)} MB/s
							</span>
						{:else}
							<span class="text-accent">{m.common_downloading()}</span>
						{/if}
					{:else}
						<span class="text-warning">{m.common_paused()}</span>
					{/if}
				</div>
			</div>
			<div class="flex items-center gap-1">
				<button
					class="btn btn-sm btn-square btn-ghost hover:text-error delete-instance-btn"
					onclick={(e) => {
						e.stopPropagation();
						showDeleteConfirm = true;
					}}
				>
					<Trash2 size={14} />
				</button>
				<InstanceMenu {instance} />
			</div>
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
				class="w-12 h-12 rounded-lg flex items-center justify-center shrink-0 overflow-hidden"
				style="background-color: {iconBg}"
			>
				{#if isSettingUp}
					<span
						class="loading loading-spinner loading-sm"
						style="color: {getContrastColor(getInstanceColor(instance))};"
					></span>
				{:else}
					<PaladinsIcon size={40} color={getContrastColor(getInstanceColor(instance))} />
				{/if}
			</div>
			<div class="flex-1 min-w-0">
				<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
					{#if instance.version}
						<span class="opacity-70 font-mono flex items-center gap-1.5">
							<Gamepad2 size={12} />
							{instance.version}
						</span>
					{/if}
					{#if isSettingUp}
						<span class="text-accent">{m.common_setting_up()}</span>
					{/if}
				</div>
			</div>
			<div class="flex items-center gap-1">
				<button
					class="btn btn-sm btn-square btn-ghost"
					disabled={isBusy}
					onclick={(e) => {
						e.stopPropagation();
						if (isRunning) {
							killMutation.mutate(instance);
						} else {
							launchMutation.mutate(instance);
						}
					}}
				>
					{#if isBusy}
						<span class="loading loading-spinner loading-xs"></span>
					{:else if isRunning}
						<Square size={14} />
					{:else}
						<Play size={14} />
					{/if}
				</button>
				<button
					class="btn btn-sm btn-square btn-ghost hover:text-error delete-instance-btn"
					onclick={(e) => {
						e.stopPropagation();
						showDeleteConfirm = true;
					}}
				>
					<Trash2 size={14} />
				</button>
				<InstanceMenu {instance} />
			</div>
		</div>
	</div>
{/if}

<DeleteInstanceDialog
	bind:open={showDeleteConfirm}
	instanceName={instance.label}
	onconfirm={handleDeleteConfirm}
/>
