<script lang="ts">
	import {
		AlertTriangle,
		ArrowDownToLine,
		CheckCircle2,
		Loader2,
		RefreshCw,
	} from "@lucide/svelte";
	import { Progress } from "bits-ui";
	import { platform } from "@tauri-apps/plugin-os";
	import { m } from "$lib/paraglide/messages";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import Modal from "../ui/Modal.svelte";

	let isOpen = $state(false);
	const isWindows = platform() === "windows";

	// Synchronize local open state with updaterStore.isOpen
	$effect(() => {
		isOpen = updaterStore.isOpen;
	});

	$effect(() => {
		updaterStore.isOpen = isOpen;
	});
</script>

<Modal bind:open={isOpen} title={m.updater_title()} class="max-w-md">
	<div class="flex flex-col gap-4 py-2">
		{#if updaterStore.status === "checking"}
			<div class="flex flex-col items-center justify-center py-6 gap-3">
				<Loader2 size={40} class="animate-spin text-accent" />
				<p class="text-sm font-medium">{m.updater_checking()}</p>
			</div>
		{:else if updaterStore.status === "up-to-date"}
			<div class="flex flex-col items-center justify-center py-6 gap-3 text-center">
				<CheckCircle2 size={48} class="text-success" />
				<h3 class="text-lg font-bold">{m.updater_uptodate_title()}</h3>
				<p class="text-sm text-base-content/70">
					{m.updater_uptodate_desc()}
				</p>
			</div>
		{:else if updaterStore.status === "available" && updaterStore.update}
			<div class="flex flex-col gap-4">
				<div class="flex items-start gap-3">
					<div class="p-3 bg-accent/10 text-accent rounded-xl shrink-0">
						<ArrowDownToLine size={24} />
					</div>
					<div>
						<h3 class="text-base font-bold">{m.updater_available_title()}</h3>
						<p class="text-sm text-base-content/60">
							{#if isWindows}
								{m.updater_available_desc_win()}
							{:else}
								{m.updater_available_desc_other()}
							{/if}
						</p>
					</div>
				</div>

				<div class="flex items-center gap-2 bg-base-200 p-3 rounded-lg text-sm">
					<span class="font-medium">{m.updater_version()}</span>
					<span class="badge badge-neutral">{updaterStore.update.currentVersion}</span>
					<span class="text-base-content/40">➔</span>
					<span class="badge badge-accent font-semibold"
						>{updaterStore.update.version}</span
					>
				</div>

				{#if updaterStore.update.body}
					<div class="flex flex-col gap-1.5">
						<span class="text-xs font-semibold text-base-content/70"
							>{m.updater_release_notes()}</span
						>
						<div
							class="bg-base-200 border border-base-300 rounded-lg p-3 max-h-32 overflow-y-auto font-sans text-xs leading-relaxed whitespace-pre-wrap"
						>
							{updaterStore.update.body}
						</div>
					</div>
				{/if}
			</div>
		{:else if updaterStore.status === "downloading" || updaterStore.status === "installing"}
			<div class="flex flex-col gap-4 py-4">
				<div class="flex items-center gap-3">
					<Loader2 size={24} class="animate-spin text-accent" />
					<div>
						<h3 class="font-bold">
							{updaterStore.status === "downloading" ?
								m.updater_downloading()
							:	m.updater_installing()}
						</h3>
						<p class="text-xs text-base-content/60">
							{#if updaterStore.status === "downloading" && updaterStore.contentLength}
								{m.updater_progress_bytes({
									received: (
										Math.round(
											(updaterStore.downloadedBytes / 1024 / 1024) * 10,
										) / 10
									).toString(),
									total: (
										Math.round(
											(updaterStore.contentLength / 1024 / 1024) * 10,
										) / 10
									).toString(),
								})}
							{:else if updaterStore.status === "downloading"}
								{m.updater_downloading_package()}
							{:else}
								{m.updater_applying_files()}
							{/if}
						</p>
					</div>
				</div>

				<Progress.Root
					value={updaterStore.contentLength ? updaterStore.progress : null}
					max={100}
					class="w-full bg-base-200 rounded-full h-2.5 overflow-hidden relative"
				>
					<div
						class="bg-accent h-full transition-all duration-300 rounded-full"
						style={updaterStore.contentLength ? `transform: translateX(-${100 - updaterStore.progress}%)` : `width: 33.333%`}
						class:animate-pulse={!updaterStore.contentLength}
					></div>
				</Progress.Root>

				{#if updaterStore.contentLength}
					<div class="flex justify-end text-xs font-mono opacity-70">
						{updaterStore.progress}%
					</div>
				{/if}
			</div>
		{:else if updaterStore.status === "relaunching"}
			<div class="flex flex-col items-center justify-center py-8 gap-3 text-center">
				<RefreshCw size={40} class="animate-spin text-accent" />
				<h3 class="text-lg font-bold">{m.updater_relaunching_title()}</h3>
				<p class="text-sm text-base-content/70">
					{m.updater_relaunching_desc()}
				</p>
			</div>
		{:else if updaterStore.status === "error"}
			<div class="flex flex-col gap-4">
				<div class="flex items-start gap-3 text-error">
					<AlertTriangle size={24} class="shrink-0 mt-0.5" />
					<div>
						<h3 class="text-base font-bold">{m.updater_failed_title()}</h3>
						<p class="text-sm text-base-content/60">{m.updater_failed_desc()}</p>
					</div>
				</div>

				{#if updaterStore.errorMessage}
					<div
						class="bg-error/10 border border-error/20 rounded-lg p-3 text-xs font-mono text-error max-h-32 overflow-y-auto"
					>
						{updaterStore.errorMessage}
					</div>
				{/if}
			</div>
		{/if}
	</div>

	{#snippet actions()}
		{#if updaterStore.status === "checking"}
			<button class="btn btn-ghost" disabled>
				{m.common_close()}
			</button>
		{:else if updaterStore.status === "relaunching"}
			<button class="btn btn-ghost" disabled>
				{m.common_close()}
			</button>
		{:else if updaterStore.status === "up-to-date"}
			<button class="btn btn-ghost" onclick={() => (isOpen = false)}>
				{m.common_close()}
			</button>
		{:else if updaterStore.status === "available" && updaterStore.update}
			{#if isWindows}
				<button class="btn btn-ghost" onclick={() => (isOpen = false)}>
					{m.updater_btn_remind()}
				</button>
				<button class="btn btn-accent gap-2" onclick={() => updaterStore.startUpdate()}>
					<ArrowDownToLine size={16} />
					{m.updater_btn_update_now()}
				</button>
			{:else}
				<button class="btn btn-ghost" onclick={() => (isOpen = false)}>
					{m.common_close()}
				</button>
			{/if}
		{:else if updaterStore.status === "downloading" || updaterStore.status === "installing"}
			<button class="btn btn-ghost" onclick={() => (isOpen = false)}>
				{m.common_close()}
			</button>
			<button class="btn btn-accent gap-2" disabled>
				<Loader2 size={16} class="animate-spin" />
				{m.updater_btn_updating()}
			</button>
		{:else if updaterStore.status === "error"}
			<button class="btn btn-ghost" onclick={() => (isOpen = false)}>
				{m.common_close()}
			</button>
			{#if isWindows}
				{#if updaterStore.update}
					<button class="btn btn-accent" onclick={() => updaterStore.startUpdate()}>
						{m.updater_btn_retry()}
					</button>
				{:else}
					<button
						class="btn btn-accent"
						onclick={() => updaterStore.checkForUpdates(false)}
					>
						{m.updater_btn_check_again()}
					</button>
				{/if}
			{:else}
				<button class="btn btn-accent" onclick={() => updaterStore.checkForUpdates(false)}>
					{m.updater_btn_check_again()}
				</button>
			{/if}
		{/if}
	{/snippet}
</Modal>
