<script lang="ts">
	import { goto } from "$app/navigation";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { createLaunchLobbyMutation } from "$lib/queries/lobby";
	import { instanceMap } from "$lib/stores/instance";
	import { servicesURL } from "$lib/stores/settings";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		open?: boolean;
	}
	//TODO check these and possibly support custom gamemodes
	type GameMode =
		| "TempestMp.Siege"
		| "TempestMp.Tdm"
		| "TempestMp.Payload"
		| "TempestMp.Onslaught";

	let { open = $bindable(false) }: Props = $props();

	const instanceList = $derived(Object.values($instanceMap).filter(Boolean) as Instance[]);

	let selectedName = $state("");
	let selectedInstanceId = $state("");
	let selectedTags = $state("");
	let selectedPassword = $state("");
	let selectedPublic = $state(false);
	let selectedGameMode = $state<GameMode>("TempestMp.Siege");
	let selectedMaxPlayers = $state<number>(10);
	let selectedMinPlayers = $state<number>(0);
	let selectedMaxSpectators = $state<number>(5);
	let selectedPort = $state<number>(50051);
	const hostLobbyMutation = createLaunchLobbyMutation();
	const hasLaunched = $derived(hostLobbyMutation.isSuccess);
	const valid = $derived(selectedName.trim().length > 0 && selectedInstanceId);

	function handleCreate() {
		const instance = instanceMap.get()[selectedInstanceId];
		if (!instance) return;
		const { path, launchOptions: options } = instance;
		const platform = options.platform ?? "Win64";
		hostLobbyMutation.mutate({
			path: path,
			name: selectedName,
			port: String(selectedPort),
			tags: selectedTags,
			version: instance.version || "?",
			"max-players": String(selectedMaxPlayers),
			"min-players": String(selectedMinPlayers),
			"public-server": selectedPublic,
			gamemode: selectedGameMode,
			password: selectedPassword || undefined,
			platform: platform,
			"no-default-args": options.noDefaultArgs,
			dll: options.dllList,
		});
		open = false;
	}
	$effect(() => {
		if (hasLaunched) {
			const pid = hostLobbyMutation.data;
			hostLobbyMutation.reset();
			goto(`/lobby-admin/${pid}`);
		}
		if (!open) {
			selectedName = "";
			selectedInstanceId = "";
			selectedGameMode = "TempestMp.Siege";
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
				<option value="TempestMp.Siege" label="Siege"></option>
				<option value="TempestMp.Payload" label="Payload"></option>
				<option value="TempestMp.Tdm" label="Team Deathmatch"></option>
				<option value="TempestMp.Onslaught" label="Onslaught"></option>
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
			<label for="server-min-players" class="label py-0.5">
				<span class="label-text text-sm">Min players to start</span>
			</label>
			<input
				id="server-min-players"
				type="number"
				class="input input-bordered w-full user-invalid:validator"
				required
				min="0"
				max="30"
				placeholder="Number between 0-30"
				bind:value={selectedMinPlayers}
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
