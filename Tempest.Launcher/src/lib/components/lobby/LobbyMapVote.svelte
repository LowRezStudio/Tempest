<script lang="ts">
	import { LogOut, Users } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import MapSelect from "../maps/MapSelect.svelte";
	import Header from "../ui/Header.svelte";

	interface Props {
		handleLeave: () => void;
		playerCount: number;
		handleMapSelect: (id: string) => void;
		votes?: Record<string, string>;
		gameVersion: string;
		gamemode: "siege" | "payload";
	}

	let { handleLeave, playerCount, handleMapSelect, votes, gameVersion, gamemode }: Props =
		$props();
</script>

<div class="relative h-full w-full">
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

	<div class="relative z-10 flex flex-col h-full">
		<Header title={m.lobby_map_vote()} class="bg-base-200/90 backdrop-blur-xs">
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
				<span>{playerCount} {m.lobby_players()}</span>
			{/snippet}
		</Header>

		<div class="min-h-0">
			<MapSelect
				onselect={(map) => handleMapSelect(map.id)}
				selectMode="vote"
				{votes}
				{gameVersion}
				{gamemode}
			/>
		</div>
	</div>
</div>
