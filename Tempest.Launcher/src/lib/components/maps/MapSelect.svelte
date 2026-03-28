<script lang="ts">
	import { Check, Map } from "@lucide/svelte";
	import maps from "$lib/data/maps.json";
	import { playerId, players } from "$lib/lobby/stores";
	import { compareVersions } from "$lib/utils/versions";

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
		gamemode: "siege" | "payload";
	}

	let { onselect, selectMode = "select", votes, gameVersion, gamemode }: Props = $props();

	let selectedMapId = $state<string | null>(null);

	const filteredMaps = maps.filter((m) => {
		return (
			compareVersions(gameVersion, m.versionStart) >= 0 &&
			compareVersions(gameVersion, m.versionEnd) <= 0 &&
			m.mode === gamemode
		);
	});

	function getVoteCount(mapId: string): number {
		if (!votes) return 0;
		return Object.values(votes).filter((v) => v === mapId).length;
	}

	function getTotalVotes(): number {
		if (!votes) return 0;
		return Object.keys(votes).length;
	}

	function getVotersForMap(mapId: string): { id: string; displayName: string }[] {
		if (!votes || !$players) return [];
		return Object.entries(votes)
			.filter(([_, votedMapId]) => votedMapId === mapId)
			.map(([voterId]) => {
				const player = $players.find((p) => p.id === voterId);
				return player ? { id: player.id, displayName: player.displayName } : null;
			})
			.filter((p): p is { id: string; displayName: string } => p !== null);
	}

	function isVotedByCurrentPlayer(mapId: string): boolean {
		if (!votes || !$playerId) return false;
		return votes[$playerId] === mapId;
	}

	function handleMapClick(map: PaladinsMap) {
		console.log("Map clicked:", map.id);
		selectedMapId = map.id;
		onselect?.(map);
	}

	$effect(() => {
		if (votes && $playerId && votes[$playerId]) {
			selectedMapId = votes[$playerId];
		}
	});
</script>

<div class="h-full w-full overflow-y-auto">
	<div
		class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3 p-4 auto-rows-fr w-full lg:px-35"
	>
		{#each filteredMaps as map (map.id)}
			{@const voteCount = getVoteCount(map.id)}
			{@const totalVotes = getTotalVotes()}
			{@const voters = getVotersForMap(map.id)}
			{@const isVotedByMe = isVotedByCurrentPlayer(map.id)}
			{@const votePercent = totalVotes > 0 ? (voteCount / totalVotes) * 100 : 0}
			<button
				type="button"
				class="group relative rounded-xl overflow-hidden bg-base-200 transition-all duration-150 cursor-pointer hover:bg-base-300 active:scale-[0.98] {(
					isVotedByMe
				) ?
					'ring-2 ring-accent'
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
						<p class="text-sm text-white/70">{map.mode}</p>
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
