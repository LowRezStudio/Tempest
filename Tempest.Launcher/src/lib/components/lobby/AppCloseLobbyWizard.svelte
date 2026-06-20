<script lang="ts">
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import { m } from "$lib/paraglide/messages";
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

<Modal bind:open title={m.close_lobby_title()} class="max-w-2xl">
	<div>
		{m.close_lobby_message()}
	</div>
	{#snippet actions()}
		<div class="flex items-center justify-end w-full">
			<div class="flex gap-2">
				<button class="btn btn-accent" onclick={handleLeaveAndClose}
					>{m.lobby_leave_lobby()}</button
				>
				<button class="btn btn-accent" onclick={handleClose}
					>{m.close_lobby_come_back()}</button
				>
			</div>
		</div>
	{/snippet}
</Modal>
