<script lang="ts">
	import { m } from "$lib/paraglide/messages";
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
		title={m.lobby_connection_lost()}
		subtitle={m.lobby_reconnecting()}
		loading
	/>
{:else if gameServerError}
	<LobbyOverlayDialog
		title={m.lobby_gameserver_crashed()}
		subtitle={m.lobby_lobby_restarting()}
		loading
	/>
{:else if joinErrorCode === JoinLobbyClientErrorCode.NO_VALID_INSTANCE}
	<LobbyOverlayDialog
		title={m.lobby_no_valid_instance({ version: lobbyVersion })}
		subtitle={m.lobby_no_valid_instance_hint()}
	/>
{:else if joinErrorCode === JoinLobbyErrorCode.LOBBY_FULL}
	<LobbyOverlayDialog
		title={freeSpaceInLobby ? m.lobby_can_be_joined() : m.lobby_is_full()}
		subtitle={freeSpaceInLobby ?
			m.lobby_players_count({ current: playerCount, max: maxPlayerCount ?? 0 })
		:	m.lobby_waiting_for_free_space({ current: playerCount, max: maxPlayerCount ?? 0 })}
		loading={!freeSpaceInLobby}
	>
		{#if freeSpaceInLobby}
			<button class="btn btn-accent" onclick={handleJoin}>{m.common_join()}</button>
		{/if}
	</LobbyOverlayDialog>
{:else if joinErrorCode === JoinLobbyClientErrorCode.KICKED}
	<LobbyOverlayDialog
		title={m.lobby_kicked()}
		subtitle={m.lobby_kicked_hint()}
	/>
{:else if joinErrorCode === JoinLobbyClientErrorCode.PASSWORD_REQUIRED || joinErrorCode === JoinLobbyErrorCode.INVALID_PASSWORD}
	<LobbyOverlayDialog
		title={joinErrorCode === JoinLobbyClientErrorCode.PASSWORD_REQUIRED ?
			m.lobby_password_required()
		:	m.lobby_incorrect_password()}
	>
		<div class="form-control">
			<input
				id="lobby-password"
				type="text"
				placeholder={m.common_password()}
				class="input input-bordered"
				bind:value={selectedPassword}
			/>
		</div>
		<button
			class="btn btn-accent"
			onclick={() => handlePasswordSubmit(selectedPassword)}
			disabled={!selectedPassword}>{m.common_join()}</button
		>
	</LobbyOverlayDialog>
{:else if joinErrorCode !== null}
	<LobbyOverlayDialog title={m.lobby_unknown_error({ code: String(joinErrorCode) })} />
{/if}
