<script lang="ts">
	import { Check } from "@lucide/svelte";
	import { playerId, players } from "$lib/lobby/stores.svelte";
	import { compareVersions, getMapsForVersion } from "$lib/utils/versions";

	interface PaladinsMap {
		id: string;
		displayName: string;
		iconPath: string;
		mode: string;
	}

	interface Props {
		onselect?: (map: PaladinsMap) => void;
		selectMode?: "vote" | "select";
		votes?: Record<string, string>;
		gameVersion: string;
		gamemode: string;
	}

	let { onselect, selectMode = "select", votes, gameVersion, gamemode }: Props = $props();

	let selectedMapId = $state<string | null>(null);
	const filteredMaps = $derived(
		getMapsForVersion(gameVersion).filter((m) => gamemode.toLowerCase().includes(m.mode)),
	);

	function getVoteCount(mapId: string): number {
		if (!votes) return 0;
		return Object.values(votes).filter((v) => v === mapId).length;
	}

	function getTotalVotes(): number {
		if (!votes) return 0;
		return Object.keys(votes).length;
	}

	function getVotersForMap(mapId: string): { id: string; displayName: string }[] {
		if (!votes || !players.value) return [];
		return Object.entries(votes)
			.filter(([_, votedMapId]) => votedMapId === mapId)
			.map(([voterId]) => {
				const player = players.value.find((p) => p.id === voterId);
				return player ? { id: player.id, displayName: player.displayName } : null;
			})
			.filter((p): p is { id: string; displayName: string } => p !== null);
	}

	function isVotedByCurrentPlayer(mapId: string): boolean {
		if (!votes || !playerId.value) return false;
		return votes[playerId.value] === mapId;
	}

	function handleMapClick(map: PaladinsMap) {
		console.log("Map clicked:", map.id);
		selectedMapId = map.id;
		onselect?.(map);
	}

	$effect(() => {
		if (votes && playerId.value && votes[playerId.value]) {
			selectedMapId = votes[playerId.value];
		}
	});
</script>

<div class="h-full w-full overflow-y-auto flex items-center justify-center">
	<div
		class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4 p-4 auto-rows-fr w-full m-auto"
	>
		{#each filteredMaps as map (map.id)}
			{@const voteCount = getVoteCount(map.id)}
			{@const totalVotes = getTotalVotes()}
			{@const voters = getVotersForMap(map.id)}
			{@const isVotedByMe = isVotedByCurrentPlayer(map.id)}
			{@const votePercent = totalVotes > 0 ? (voteCount / totalVotes) * 100 : 0}
			<button
				type="button"
				class="group relative rounded-none overflow-hidden bg-base-200 transition-all duration-150 cursor-pointer hover:bg-base-300 active:scale-[0.98] {(
					isVotedByMe
				) ?
					'ring-4 ring-accent'
				:	''}"
				onclick={() => handleMapClick(map)}
			>
				<div class="aspect-[16/10] relative overflow-hidden">
					<img
						src={map.iconPath}
						alt={map.displayName}
						class="w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
						loading="lazy"
					/>
					<div
						class="absolute inset-0 bg-gradient-to-t from-black/80 via-black/20 to-transparent"
					></div>

					{#if selectMode === "vote" && voteCount > 0}
						<div class="absolute top-2 left-2">
							<div
								class="bg-black/60 backdrop-blur-xs rounded-full px-2 py-1 flex items-center gap-1"
							>
								<span class="text-white text-xs font-bold">{voteCount}</span>
							</div>
						</div>
					{/if}

					{#if isVotedByMe}
						<div class="absolute top-2 right-2">
							<div class="bg-accent text-accent-content rounded-full p-1">
								<Check size={12} />
							</div>
						</div>
					{/if}

					<div class="absolute bottom-0 left-0 right-0 p-3">
						<p class="font-bold text-white text-base truncate">{map.displayName}</p>
						<p class="text-xs font-bold text-white/70 uppercase">{map.mode}</p>
					</div>
				</div>

				{#if selectMode === "vote"}
					<div class="relative h-1 bg-base-300">
						<div
							class="absolute inset-y-0 left-0 bg-accent transition-all duration-300"
							style="width: {votePercent}%"
						></div>
					</div>
					<div class="h-10 p-2 flex items-center gap-1.5 bg-base-300/50">
						{#if voters.length > 0}
							{#each voters.slice(0, 5) as voter (voter.id)}
								<div
									class="w-6 h-6 rounded-full bg-base-300 flex items-center justify-center text-xs font-semibold"
									title={voter.displayName}
								>
									{voter.displayName.charAt(0).toUpperCase()}
								</div>
							{/each}
							{#if voters.length > 5}
								<span class="text-xs opacity-70 ml-1">+{voters.length - 5}</span>
							{/if}
						{/if}
					</div>
				{/if}
			</button>
		{/each}
	</div>
</div>
