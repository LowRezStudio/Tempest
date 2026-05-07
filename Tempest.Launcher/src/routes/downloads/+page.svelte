<script lang="ts">
	import { Download, FolderOpen, Pause, Play, Plus, RotateCcw, Trash2 } from "@lucide/svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import {
		queueCompletedCount,
		queueErrorCount,
		queueItems,
		queuePendingCount,
		queueRunning,
		resetQueueState,
	} from "$lib/rigby/stores";

	function formatBytes(bytes: number): string {
		if (bytes === 0) return "0 B";
		const k = 1024;
		const sizes = ["B", "KB", "MB", "GB", "TB"];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return `${Number.parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
	}

	function formatSpeed(bytesPerSecond: number): string {
		return `${formatBytes(bytesPerSecond)}/s`;
	}

	function formatTime(seconds: number): string {
		if (seconds < 60) return `${Math.round(seconds)}s`;
		if (seconds < 3600) {
			const mins = Math.floor(seconds / 60);
			const secs = Math.round(seconds % 60);
			return `${mins}m ${secs}s`;
		}
		const hours = Math.floor(seconds / 3600);
		const mins = Math.floor((seconds % 3600) / 60);
		return `${hours}h ${mins}m`;
	}

	function handleStart(): void {
		restoreQueue.start();
	}

	function handlePause(): void {
		restoreQueue.pause();
	}

	function handleClearCompleted(): void {
		restoreQueue.clearCompleted();
	}

	function handleRemove(id: string): void {
		restoreQueue.remove(id);
	}

	function statusText(status: string): string {
		if (status === "pending") return m.downloads_pending();
		if (status === "complete") return m.downloads_complete();
		if (status === "error") return m.downloads_failed();
		return status;
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header title={m.downloads_title()}>
		{#snippet icon()}
			<Download size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
			{#if $queueRunning}
				<button class="btn btn-ghost" onclick={handlePause}>
					<Pause size={16} />
					{m.common_pause()}
				</button>
			{:else}
				<button
					class="btn btn-ghost"
					onclick={handleStart}
					disabled={$queuePendingCount === 0}
				>
					<Play size={16} />
					{m.common_start()}
				</button>
			{/if}
			<button
				class="btn btn-ghost"
				onclick={handleClearCompleted}
				disabled={$queueCompletedCount === 0 && $queueErrorCount === 0}
			>
				<Trash2 size={16} />
				{m.downloads_clear_completed()}
			</button>
		{/snippet}
		{#snippet subtitle()}
			{#if $queuePendingCount > 0}
				<span class="badge badge-accent badge-sm"
					>{$queuePendingCount} {m.downloads_pending()}</span
				>
			{/if}
			{#if $queueCompletedCount > 0}
				<span class="badge badge-success badge-sm"
					>{$queueCompletedCount} {m.downloads_complete()}</span
				>
			{/if}
			{#if $queueErrorCount > 0}
				<span class="badge badge-error badge-sm"
					>{$queueErrorCount} {m.downloads_failed()}</span
				>
			{/if}
			{#if $queueItems.length === 0}
				<span>{m.downloads_no_downloads()}</span>
			{/if}
		{/snippet}
	</Header>
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				{#if $queueItems.length === 0}
					<div class="flex flex-col items-center justify-center h-64 gap-4">
						<FolderOpen size={48} class="opacity-30" />
						<p class="text-lg text-base-content/50">{m.downloads_no_downloads()}</p>
						<p class="text-sm text-base-content/40">
							{m.downloads_no_downloads_hint()}
						</p>
					</div>
				{:else}
					<div class="space-y-2">
						{#each $queueItems as item (item.id)}
							<div class="bg-base-200 rounded-lg p-4">
								<div class="flex items-start justify-between gap-4">
									<div class="flex-1 min-w-0">
										<div class="flex items-center gap-2 mb-2">
											<span
												class="badge badge-sm {item.status === 'pending' ?
													'badge-neutral'
												: item.status === 'running' ? 'badge-accent'
												: item.status === 'complete' ? 'badge-success'
												: 'badge-error'}"
											>
												{statusText(item.status)}
											</span>
											<span
												class="text-sm font-mono truncate opacity-70"
												title={item.outDir}
											>
												{item.outDir.split("/").pop() || item.outDir}
											</span>
										</div>

										{#if item.status === "running" && item.progress}
											{@const progress = item.progress}
											<div class="space-y-2">
												<div
													class="flex items-center justify-between text-sm"
												>
													<span class="font-semibold"
														>{progress.percent.toFixed(1)}%</span
													>
													<span class="opacity-70">
														{progress.completedFiles}/{progress.totalFiles}
														{m.downloads_files()}
													</span>
												</div>
												<progress
													class="progress progress-accent w-full"
													value={progress.percent}
													max="100"
												></progress>
												<div
													class="flex items-center justify-between text-xs opacity-70"
												>
													<span
														>{formatSpeed(
															progress.bytesPerSecond,
														)}</span
													>
													<span>
														{formatBytes(progress.completedBytes)} / {formatBytes(
															progress.totalBytes,
														)}
													</span>
													{#if progress.etaSeconds > 0}
														<span
															>{m.downloads_eta()}: {formatTime(
																progress.etaSeconds,
															)}</span
														>
													{:else}
														<span>{m.common_calculating()}</span>
													{/if}
												</div>
											</div>
										{:else if item.status === "complete" && item.result}
											<div class="text-sm space-y-1">
												<div class="flex items-center gap-4">
													<span class="text-success font-semibold"
														>{item.result.files}
														{m.downloads_files()}</span
													>
													<span class="opacity-70"
														>{formatBytes(item.result.diskWriteBytes)}
														{m.downloads_written()}</span
													>
												</div>
												{#if item.result.repairedFiles > 0}
													<span class="text-warning text-xs"
														>{item.result.repairedFiles}
														{m.downloads_repaired()}</span
													>
												{/if}
												{#if item.result.verifiedFiles > 0}
													<span class="text-xs opacity-60"
														>{item.result.verifiedFiles}
														{m.downloads_verified()}</span
													>
												{/if}
											</div>
										{:else if item.status === "error" && item.error}
											<p class="text-sm text-error">{item.error}</p>
										{:else if item.status === "pending"}
											<p class="text-sm opacity-50">
												{m.common_waiting_in_queue()}
											</p>
										{/if}
									</div>

									<div class="flex items-center gap-1">
										{#if item.status === "running"}
											<button
												class="btn btn-ghost btn-sm btn-square"
												onclick={handlePause}
												aria-label={m.common_pause()}
											>
												<Pause size={16} />
											</button>
										{:else if item.status === "pending" || item.status === "error" || item.status === "complete"}
											<button
												class="btn btn-ghost btn-sm btn-square"
												onclick={() => handleRemove(item.id)}
												aria-label={m.common_remove()}
											>
												<Trash2 size={16} />
											</button>
										{/if}
									</div>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>
