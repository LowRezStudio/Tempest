<script lang="ts">
	import { m } from "$lib/paraglide/messages";
	import LobbyOverlayDialog from "./LobbyOverlayDialog.svelte";

	interface Props {
		runAfkDetection: boolean;
		onAfk: () => void;
	}

	let { runAfkDetection, onAfk }: Props = $props();
	let isAfk = $state<boolean>(false);

	$effect(() => {
		if (runAfkDetection && !isAfk) {
			const id = setTimeout(
				() => {
					isAfk = true;
				},
				5 * 60 * 1000,
			);
			return () => clearTimeout(id);
		}
	});
	$effect(() => {
		if (isAfk) {
			const id = setTimeout(() => {
				onAfk();
			}, 60 * 1000);
			return () => clearTimeout(id);
		}
	});
	$effect(() => {
		if (!runAfkDetection) {
			isAfk = false;
		}
	});
	function handleCancelAfk() {
		isAfk = false;
	}
</script>

{#if isAfk}
	<LobbyOverlayDialog title={m.afk_title()}>
		<button class="btn btn-accent" onclick={handleCancelAfk}>{m.common_yes()}</button>
	</LobbyOverlayDialog>
{/if}
