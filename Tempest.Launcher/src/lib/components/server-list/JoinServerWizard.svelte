<script lang="ts">
	import Modal from "$lib/components/ui/Modal.svelte";
	import { moveToLobby } from "$lib/core/lobby";
	import { m } from "$lib/paraglide/messages";
	import { isValidHost } from "$lib/utils/network";

	interface Props {
		open?: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	let selectedHost = $state<string>("");
	let selectedPort = $state<number>(50051);

	const valid = $derived(isValidHost(selectedHost));

	function handleJoin() {
		if (!valid) return;
		moveToLobby(`http://${selectedHost}:${selectedPort}`);
		open = false;
	}
</script>

<Modal bind:open title={m.join_server_title()} class="max-w-2xl" onsubmit={handleJoin}>
	<div class="space-y-4">
		<div class="form-control">
			<label for="host" class="label py-0.5">
				<span class="label-text text-sm">{m.join_server_host()}</span>
			</label>
			<input
				id="host"
				type="text"
				placeholder={m.join_server_host_placeholder()}
				maxlength="60"
				class="input input-bordered w-full"
				bind:value={selectedHost}
			/>
		</div>
		<div class="form-control">
			<label for="server-port" class="label py-0.5">
				<span class="label-text text-sm">{m.join_server_port()}</span>
			</label>
			<input
				id="server-port"
				type="number"
				class="input input-bordered w-full user-invalid:validator"
				required
				min="50000"
				max="65000"
				placeholder={m.join_server_port_placeholder()}
				bind:value={selectedPort}
			/>
		</div>
	</div>

	{#snippet actions()}
		<div class="flex items-center justify-end w-full">
			<div class="flex gap-2">
				<button class="btn btn-ghost" type="button" onclick={() => (open = false)}
					>{m.common_cancel()}</button
				>
				<button class="btn btn-accent" type="submit" disabled={!valid}>
					{m.common_join()}
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
