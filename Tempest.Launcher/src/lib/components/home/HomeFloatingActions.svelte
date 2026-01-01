<script lang="ts">
	import { Box, Play, Megaphone, Square } from "@lucide/svelte";
	import { lastLaunchedInstance, lastLaunchedInstanceId } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { launchGame, killGame } from "$lib/core";

	// Mock announcement data
	const announcement = {
		title: "Announcement",
		message: "Tempest Launcher is still in development so apologies if there's any bugs.",
	};

	// Check if the last launched instance is currently running
	let isRunning = $derived(
		$lastLaunchedInstance ?
			$processesList.some((p) => p.instance.id === $lastLaunchedInstance.id)
		:	false,
	);
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
					<h3 class="font-bold text-sm">{announcement.title}</h3>
				</div>
				<p class="text-xs opacity-90 leading-relaxed">
					{announcement.message}
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
				onclick={() =>
					isRunning ? killGame($lastLaunchedInstance) : launchGame($lastLaunchedInstance)}
				aria-label={isRunning ? "Stop game" : "Launch game"}
			>
				{#if isRunning}
					<Square size={24} />
				{:else}
					<Play size={24} />
				{/if}
				<div class="flex flex-col items-start">
					<span class="font-semibold text-sm">{isRunning ? "Stop Game" : "Run Game"}</span
					>
					<span class="text-xs opacity-80">{$lastLaunchedInstance.label}</span>
				</div>
			</button>

			<a href={`/instance/${$lastLaunchedInstanceId}`} class="btn btn-lg join-item">
				<Box size={20} />
			</a>
		</div>
	{/if}
</div>
