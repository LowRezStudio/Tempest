<script lang="ts">
	import { Eye, EyeOff } from "@lucide/svelte";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import { m } from "$lib/paraglide/messages";
	import { isSpectator, isWaiting, players, playerId, spectators } from "$lib/lobby/stores.svelte";

	const ownPlayer = $derived(players.value.find((p) => p.id === playerId.value));

	function handleToggle() {
		if (isSpectator.value) {
			// rejoin the smaller team
			const left = players.value.filter((p) => p.taskForce === 1).length;
			const right = players.value.filter((p) => p.taskForce === 2).length;
			lobbyManager.switchTeam(left <= right ? 1 : 2);
		} else {
			lobbyManager.switchTeam(0);
		}
	}
</script>

<div class="pointer-events-none flex flex-col items-end gap-2">
	{#if spectators.value.length > 0}
		<div class="bg-base-200/95 backdrop-blur-xs rounded-lg shadow-xl px-3 py-2 max-w-52 text-center">
			<p class="text-xs opacity-60 uppercase tracking-wide mb-1">{m.lobby_spectators()}</p>
			<ul class="flex flex-col gap-0.5 text-sm">
				{#each spectators.value as spectator (spectator.id)}
					<li class="truncate">
						{spectator.displayName}
						{#if spectator.id === playerId.value}
							<span class="opacity-60">({m.common_you()})</span>
						{/if}
					</li>
				{/each}
			</ul>
		</div>
	{/if}
	{#if ownPlayer && isWaiting.value}
		<button class="btn btn-sm shadow-none pointer-events-auto" onclick={handleToggle}>
			{#if isSpectator.value}
				<EyeOff size={16} />
				{m.lobby_join_team()}
			{:else}
				<Eye size={16} />
				{m.lobby_become_spectator()}
			{/if}
		</button>
	{/if}
</div>
