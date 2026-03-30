<script lang="ts">
	import { JoinLobbyErrorCode } from "$lib/rpc/lobby/join_lobby_error_code";
	import { JoinLobbyClientErrorCode } from "$lib/types/lobby";
	import LobbyOverlayDialog from "./LobbyOverlayDialog.svelte";
	import type { ExtendedJoinLobbyErrorCode } from "$lib/types/lobby";

	interface Props {
		disconnected: boolean;
		gameServerError: boolean;
		joinErrorCode: ExtendedJoinLobbyErrorCode | null;
		handlePasswordSubmit: (password: string) => void;
		handleJoin: () => void;
		lobbyVersion: string;
		playerCount: number;
		maxPlayerCount: number | null;
	}

	let {
		disconnected,
		gameServerError,
		joinErrorCode,
		handlePasswordSubmit,
		handleJoin,
		lobbyVersion,
		playerCount,
		maxPlayerCount,
	}: Props = $props();

	let selectedPassword = $state<string>("");
	const freeSpaceInLobby = $derived(maxPlayerCount !== null && playerCount < maxPlayerCount);
</script>

{#if disconnected}
	<LobbyOverlayDialog
		title="Connection Lost"
		subtitle="Unable to connect to the lobby server. Reconnecting..."
		loading
	/>
{:else if gameServerError}
	<LobbyOverlayDialog title="Gameserver Crashed" subtitle="Lobby is restarting..." loading />
{:else if joinErrorCode === JoinLobbyClientErrorCode.NO_VALID_INSTANCE}
	<LobbyOverlayDialog
		title={`No valid instance found for version ${lobbyVersion}`}
		subtitle="Please leave the lobby and install the correct game version"
	/>
{:else if joinErrorCode === JoinLobbyErrorCode.LOBBY_FULL}
	<LobbyOverlayDialog
		title={freeSpaceInLobby ? "Lobby can be joined" : "Lobby is full"}
		subtitle={freeSpaceInLobby ?
			`Players ${playerCount}/${maxPlayerCount}`
		:	`Waiting for free space (${playerCount}/${maxPlayerCount})`}
		loading={!freeSpaceInLobby}
	>
		{#if freeSpaceInLobby}
			<button class="btn btn-accent" onclick={handleJoin}>Join</button>
		{/if}
	</LobbyOverlayDialog>
{:else if joinErrorCode === JoinLobbyClientErrorCode.PASSWORD_REQUIRED || joinErrorCode === JoinLobbyErrorCode.INVALID_PASSWORD}
	<LobbyOverlayDialog
		title={joinErrorCode === JoinLobbyClientErrorCode.PASSWORD_REQUIRED ?
			"Password Required"
		:	"Incorrect Password. Please Try Again."}
	>
		<div class="form-control">
			<input
				id="lobby-password"
				type="text"
				placeholder="Password"
				class="input input-bordered"
				bind:value={selectedPassword}
			/>
		</div>
		<button
			class="btn btn-accent"
			onclick={() => handlePasswordSubmit(selectedPassword)}
			disabled={!selectedPassword}>Join</button
		>
	</LobbyOverlayDialog>
{:else if joinErrorCode !== null}
	<LobbyOverlayDialog title="Unknown Error" />
{/if}
