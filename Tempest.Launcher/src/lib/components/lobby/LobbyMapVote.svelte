<script lang="ts">
	import { LogOut, Users } from "@lucide/svelte";
	import { onMount } from "svelte";
	import champions from "$lib/data/champions.json";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import { m } from "$lib/paraglide/messages";
	import { getMapsForVersion } from "$lib/utils/versions";
	import MapSelect from "../maps/MapSelect.svelte";
	import Header from "../ui/Header.svelte";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";

	interface Props {
		handleLeave: () => void;
		playerCount: number;
		handleMapSelect: (id: string) => void;
		votes?: Record<string, string>;
		gameVersion: string;
		gamemode: string;
		countdownSeconds: number;
	}

	let {
		handleLeave,
		playerCount,
		handleMapSelect,
		votes,
		gameVersion,
		gamemode,
		countdownSeconds,
	}: Props = $props();

	function getChampionDisplayName(champion: string | undefined): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}

	let votedCount = $derived(Object.keys(votes ?? {}).length);

	onMount(() => {
		const defaultMap = getMapsForVersion(gameVersion).find((m) =>
			gamemode.toLowerCase().includes(m.mode),
		);
		if (defaultMap) {
			lobbyManager.setDefaultMap(defaultMap.id);
		}
	});
</script>

<div class="relative flex h-full w-full flex-col">
	<div class="absolute inset-0">
		<video
			src="/champions/empty.webm"
			class="h-full w-full object-cover blur-xs"
			loop
			muted
			playsinline
			autoplay
		></video>
	</div>

	<Header
		title={countdownSeconds > 0
			? `${m.lobby_map_vote()} ${countdownSeconds}s`
			: m.lobby_map_vote()}
		class="bg-base-200/90 relative z-[60] backdrop-blur-xs"
	>
		{#snippet icon()}
			<Users size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
			<button class="btn btn-error" onclick={handleLeave}>
				<LogOut size={18} />
				{m.lobby_leave_lobby()}
			</button>
		{/snippet}
		{#snippet subtitle()}
			<span>{gameVersion}</span>
			<span class="opacity-30">|</span>
			<span>{m.lobby_map_votes({ voted: votedCount, total: playerCount })}</span>
		{/snippet}
	</Header>

	<div class="relative z-20 flex min-h-0 w-full flex-1 flex-col">
		<div class="min-h-0 w-full flex-1">
			<MapSelect
				onselect={(map) => handleMapSelect(map.id)}
				selectMode="vote"
				{votes}
				{gameVersion}
				{gamemode}
			/>
		</div>
	</div>

	<!-- Left Side Panel (Blue Gradient) -->
	<div
		class="absolute top-0 bottom-0 left-0 z-10 flex w-48 flex-col gap-3 bg-gradient-to-r from-blue-950/95 via-blue-900/40 to-transparent p-4 pt-16 md:w-64 md:p-6 lg:w-80"
	></div>

	<!-- Right Side Panel (Red Gradient) -->
	<div
		class="absolute top-0 right-0 bottom-0 z-10 flex w-48 flex-col gap-3 bg-gradient-to-l from-red-950/95 via-red-900/40 to-transparent p-4 pt-16 md:w-64 md:p-6 lg:w-80"
	></div>
</div>
