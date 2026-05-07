<script lang="ts">
	import { Box, Megaphone, Play, Square } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { lastLaunchedInstance, lastLaunchedInstanceId } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";

	let isRunning = $derived(
		$lastLaunchedInstance ?
			$processesList.some((p) => p.instance.id === $lastLaunchedInstance.id)
		:	false,
	);

	const launchGameMutation = createLaunchGameMutation();
	const killGameMutation = createKillGameMutation();
	let isLaunching = $derived(launchGameMutation.isPending);
	let isKilling = $derived(killGameMutation.isPending);
	let actionError = $derived(
		launchGameMutation.error?.message || killGameMutation.error?.message || "",
	);

	function handleLaunchToggle() {
		if (!$lastLaunchedInstance) return;
		if (isRunning) {
			killGameMutation.mutate($lastLaunchedInstance);
			return;
		}
		launchGameMutation.mutate($lastLaunchedInstance);
	}

	function clearActionError() {
		if (launchGameMutation.error) {
			launchGameMutation.reset();
		}
		if (killGameMutation.error) {
			killGameMutation.reset();
		}
	}
</script>

<div class="fixed bottom-6 left-6 right-6 z-50 flex items-end justify-between gap-6">
	<div class="max-w-xl">
		<div class="card bg-base-200/95 shadow-xl backdrop-blur-sm">
			<div class="card-body p-4">
				<div class="flex items-center gap-3 mb-2">
					<div
						class="w-10 h-10 rounded-lg bg-base-300 flex items-center justify-center shrink-0"
					>
						<Megaphone size={20} class="opacity-60" />
					</div>
					<h3 class="font-bold text-sm">{m.home_announcement_title()}</h3>
				</div>
				<p class="text-xs opacity-90 leading-relaxed">
					{m.home_announcement_message()}
				</p>
			</div>
		</div>
	</div>

	{#if $lastLaunchedInstance}
		<div class="join shadow-lg">
			<button
				class="btn btn-lg join-item gap-2"
				class:btn-accent={!isRunning}
				class:btn-error={isRunning}
				disabled={isLaunching || isKilling}
				aria-busy={isLaunching || isKilling}
				onclick={handleLaunchToggle}
				aria-label={isRunning ? m.home_stop_game() : m.home_run_game()}
			>
				{#if isLaunching}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if isRunning}
					<Square size={24} />
				{:else}
					<Play size={24} />
				{/if}
				<div class="flex flex-col items-start">
					<span class="font-semibold text-sm">
						{isLaunching ? m.common_launching()
						: isKilling ? m.common_stopping_label()
						: isRunning ? m.home_stop_game()
						: m.home_run_game()}
					</span>
					<span class="text-xs opacity-80">{$lastLaunchedInstance.label}</span>
				</div>
			</button>

			<a href={`/instance/${$lastLaunchedInstanceId}`} class="btn btn-lg join-item">
				<Box size={20} />
			</a>
		</div>
		{#if actionError}
			<div class="pt-2">
				<div class="alert alert-error">
					<span>{actionError}</span>
					<button class="btn btn-ghost btn-sm" onclick={clearActionError}
						>{m.common_dismiss()}</button
					>
				</div>
			</div>
		{/if}
	{/if}
</div>
