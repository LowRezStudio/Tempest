<script lang="ts">
	import Modal from "$lib/components/ui/Modal.svelte";

	interface Props {
		open?: boolean;
	}
	type GameMode = "siege" | "tdm" | "payload" | "onslaught";
	import type { Instance } from "$lib/types/instance";

	let { open = $bindable(false) }: Props = $props();
	import { instanceMap } from "$lib/stores/instance";
	import { createLaunchLobbyMutation } from "$lib/queries/lobby";
	const instanceList = $derived(Object.values($instanceMap).filter(Boolean) as Instance[]);

	let selectedName = $state("");
	let selectedInstanceId = $state("");
	let selectedTags = $state("");
	let selectedPassword = $state("");
	let selectedPublic = $state(false);
	let selectedGameMode = $state<GameMode>("siege");
	let selectedMaxPlayers = $state<number>(10);
	let selectedMaxSpectators = $state<number>(5);
	let selectedPort = $state<number>(50051);
	const hostLobbyMutation = createLaunchLobbyMutation();
	const valid = $derived(selectedName.trim().length > 0 && selectedInstanceId);

	function handleCreate() {
		hostLobbyMutation.mutate({
			name: selectedName,
			port: selectedPort + "",
			tags: selectedTags,
			version: instanceMap.get()[selectedInstanceId].version || "?",
			"max-players": selectedMaxPlayers + "",
			"public-server": selectedPublic,
			gamemode: selectedGameMode,
			password: selectedPassword || undefined,
		});
		open = false;
	}
	$effect(() => {
		if (!open) {
			selectedName = "";
			selectedInstanceId = "";
			selectedGameMode = "siege";
			selectedPublic = false;
			selectedPassword = "";
			selectedTags = "";
			selectedMaxPlayers = 10;
			selectedMaxSpectators = 5;
			selectedPort = 50051;
		}
	});
</script>

<Modal bind:open title="Host server" class="max-w-2xl">
	<div class="space-y-4">
		<div class="form-control">
			<label for="server-name" class="label py-0.5">
				<span class="label-text text-sm">Server name</span>
			</label>
			<input
				id="server-name"
				type="text"
				required
				class="input input-bordered w-full user-invalid:validator"
				minlength="3"
				maxlength="30"
				bind:value={selectedName}
			/>
		</div>
		<div class="form-control">
			<label for="instance" class="label py-0.5">
				<span class="label-text text-sm">Instance</span>
			</label>
			<select
				id="instance"
				class="select select-bordered w-full"
				bind:value={selectedInstanceId}
			>
				<option value="" disabled>Select an instance...</option>
				{#each instanceList as instance (instance.id)}
					<option
						value={instance.id}
						label={instance.label + " version: " + instance.version}
					></option>
				{/each}
			</select>
		</div>
		<div class="form-control">
			<label for="gamemode" class="label py-0.5">
				<span class="label-text text-sm">Gamemode</span>
			</label>
			<select
				id="gamemode"
				class="select select-bordered w-full"
				bind:value={selectedGameMode}
			>
				<option value="siege" label="Siege"></option>
				<option value="tdm" label="Team Deathmatch"></option>
				<option value="payload" label="Payload"></option>
				<option value="onslaught" label="Onslaught"></option>
			</select>
		</div>
		<span class="label-text text-sm label">Public servers are visible on the server list</span>
		<div class="form-control flex items-center justify-between">
			<label for="public" class="label">
				<span class="label-text text-sm">Public</span>
			</label>
			<input id="public" class="toggle" type="checkbox" bind:checked={selectedPublic} />
			<label for="password" class="label">
				<span class="label-text text-sm">Password</span>
			</label>
			<input
				id="password"
				type="text"
				placeholder="Leave empty for no password"
				maxlength="60"
				class="input input-bordered"
				bind:value={selectedPassword}
			/>
		</div>
		<div class="form-control">
			<label for="server-tags" class="label py-0.5">
				<span class="label-text text-sm">Tags separated with commas</span>
				<span class="label-text-alt text-xs">Optional</span>
			</label>
			<input
				id="server-tags"
				type="text"
				maxlength="30"
				class="input input-bordered w-full"
				bind:value={selectedTags}
			/>
		</div>
		<div class="form-control">
			<label for="server-max-players" class="label py-0.5">
				<span class="label-text text-sm">Max players</span>
			</label>
			<input
				id="server-max-players"
				type="number"
				class="input input-bordered w-full user-invalid:validator"
				required
				min="1"
				max="30"
				placeholder="Number between 1-30"
				bind:value={selectedMaxPlayers}
			/>
		</div>
		<div class="form-control">
			<label for="server-max-spectators" class="label py-0.5">
				<span class="label-text text-sm">Max spectators</span>
			</label>
			<input
				id="server-max-spectators"
				type="number"
				class="input input-bordered w-full user-invalid:validator"
				required
				min="1"
				max="30"
				placeholder="Number between 1-30"
				bind:value={selectedMaxSpectators}
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
				<button class="btn btn-accent" disabled={!valid} onclick={handleCreate}>
					Create
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
