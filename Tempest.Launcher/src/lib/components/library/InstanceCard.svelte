<script lang="ts">
	import { goto } from "$app/navigation";
	import { Gamepad2, Play, Square, Trash2 } from "@lucide/svelte";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { deleteInstance } from "$lib/core/instance-delete";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchGameMutation, createKillGameMutation } from "$lib/queries/core";
	import { queueItems } from "$lib/rigby/stores.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
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
			(item) =>
				item.outDir === instance.path &&
				(item.status === "running" || item.status === "pending"),
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
	<div class="bg-base-200 relative overflow-hidden rounded-lg p-4 opacity-80">
		<div class="flex items-center gap-3">
			<div
				class="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg"
				style="background-color: {iconBg}"
			>
				{#if isDownloading}
					<span
						class="text-xs font-bold tabular-nums"
						style="color: {getContrastColor(iconBg)};"
						>{Math.round(downloadProgress)}%</span
					>
				{:else}
					<PaladinsIcon size={40} color={getContrastColor(iconBg)} />
				{/if}
			</div>
			<div class="min-w-0 flex-1">
				<h3 class="mb-0.5 truncate text-base font-bold">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
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

		<div
			class="absolute bottom-0 left-0 h-1 rounded-full"
			style="width: {downloadProgress}%; background-color: {iconBg}; transition: width 0.3s ease;"
		></div>
	</div>
{:else}
	<div
		class="bg-base-200 hover:bg-base-300 cursor-pointer rounded-lg p-4 text-left transition-all duration-200"
		onclick={handleCardClick}
		role="link"
		tabindex="0"
		onkeydown={(e) => {
			if (e.key === "Enter" || e.key === " ") handleCardClick(e as unknown as MouseEvent);
		}}
	>
		<div class="flex items-center gap-3">
			<div
				class="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg"
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
			<div class="min-w-0 flex-1">
				<h3 class="mb-0.5 truncate text-base font-bold">{instance.label}</h3>
				<div class="flex items-center gap-2 text-sm">
					{#if instance.version}
						<span class="flex items-center gap-1.5 font-mono opacity-70">
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
