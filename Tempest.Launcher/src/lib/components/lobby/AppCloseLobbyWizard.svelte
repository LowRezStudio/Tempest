<script lang="ts">
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import Modal from "../ui/Modal.svelte";

	interface Props {
		open?: boolean;
	}
	let { open = $bindable(false) }: Props = $props();

	async function handleLeaveAndClose() {
		if (lobbyManager.isConnected()) {
			await lobbyManager.leaveLobby();
		}
		await handleClose();
	}
	async function handleClose() {
		const appWindow = getCurrentWindow();
		await appWindow.destroy();
	}
</script>

<Modal bind:open title="In a lobby" class="max-w-2xl">
	<div>
		You are connected to a lobby and trying to close the application. Would you like to leave
		the lobby or do you plan on coming back?
	</div>
	{#snippet actions()}
		<div class="flex items-center justify-end w-full">
			<div class="flex gap-2">
				<button class="btn btn-accent" onclick={handleLeaveAndClose}>Leave Lobby</button>
				<button class="btn btn-accent" onclick={handleClose}>I'll come back soon</button>
			</div>
		</div>
	{/snippet}
</Modal>
