<script lang="ts">
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
	<LobbyOverlayDialog title="Are you still there?">
		<button class="btn btn-accent" onclick={handleCancelAfk}>YES</button>
	</LobbyOverlayDialog>
{/if}
