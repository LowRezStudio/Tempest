<script lang="ts">
	import Modal from "$lib/components/ui/Modal.svelte";
	import { moveToLobby } from "$lib/core/lobby";

	interface Props {
		open?: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	let selectedHost = $state<string>("");
	let selectedPort = $state<number>(50051);

	const valid = $derived(selectedHost.trim().length > 0);

	function handleJoin() {
		moveToLobby(`http://${selectedHost}:${selectedPort}`);
		open = false;
	}
</script>

<Modal bind:open title="Join server" class="max-w-2xl">
	<div class="space-y-4">
		<div class="form-control">
			<label for="host" class="label py-0.5">
				<span class="label-text text-sm">Host</span>
			</label>
			<input
				id="host"
				type="text"
				placeholder="IP address or domain"
				maxlength="60"
				class="input input-bordered w-full"
				bind:value={selectedHost}
			/>
		</div>
		<div class="form-control">
			<label for="server-port" class="label py-0.5">
				<span class="label-text text-sm">Port</span>
			</label>
			<input
				id="server-port"
				type="number"
				class="input input-bordered w-full user-invalid:validator"
				required
				min="50000"
				max="65000"
				placeholder="Port between 50000-65000"
				bind:value={selectedPort}
			/>
		</div>
	</div>

	{#snippet actions()}
		<div class="flex items-center justify-end w-full">
			<div class="flex gap-2">
				<button class="btn btn-ghost" onclick={() => (open = false)}>Cancel</button>
				<button class="btn btn-accent" disabled={!valid} onclick={handleJoin}>
					Join
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
