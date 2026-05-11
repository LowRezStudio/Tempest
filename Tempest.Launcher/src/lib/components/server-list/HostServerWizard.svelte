<script lang="ts">
	import { Server, SlidersHorizontal } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchLobbyMutation } from "$lib/queries/lobby";
	import { instanceMap } from "$lib/stores/instance";
	import { servicesURL, username } from "$lib/stores/settings";
	import { untrack } from "svelte";
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

	let selectedTab = $state<"general" | "advanced">("general");
	let selectedName = $state($username ? `${$username}'s Server` : "");
	let selectedInstanceId = $state(
		untrack(() => (instanceList.length === 1 ? instanceList[0].id : "")),
	);
	let selectedTags = $state("");
	let selectedPassword = $state("");
	let selectedPublic = $state(false);
	let selectedGameMode = $state<GameMode>("TempestMp.Siege");
	let selectedMaxPlayers = $state<number>(10);
	let selectedMinPlayers = $state<number>(1);
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
			//currently hard coded to false because the server preloads the champions
			"enable-joining-mid-game": false,
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
			selectedTab = "general";
			selectedName = $username ? `${$username}'s Server` : "";
			selectedInstanceId = instanceList.length === 1 ? instanceList[0].id : "";
			selectedGameMode = "TempestMp.Siege";
			selectedPublic = false;
			selectedPassword = "";
			selectedTags = "";
			selectedMaxPlayers = 10;
			selectedMinPlayers = 1;
			selectedMaxSpectators = 5;
			selectedPort = 50051;
		}
	});
</script>

<Modal bind:open title={m.hostserver_title()} class="max-w-2xl">
	<div role="tablist" class="tabs tabs-border w-full mb-4">
		<button
			role="tab"
			class={["tab gap-2 flex-1", selectedTab === "general" && "tab-active"]}
			onclick={() => (selectedTab = "general")}
		>
			<Server size={16} />
			<span>General</span>
		</button>
		<button
			role="tab"
			class={["tab gap-2 flex-1", selectedTab === "advanced" && "tab-active"]}
			onclick={() => (selectedTab = "advanced")}
		>
			<SlidersHorizontal size={16} />
			<span>Advanced</span>
		</button>
	</div>
	<!--Rendering both tabs always so the height of the modal does not change-->
	<div class="grid">
		<!-- General tab -->
		<div
			class={[
				"space-y-4 row-start-1 col-start-1",
				selectedTab !== "general" && "invisible pointer-events-none",
			]}
		>
			<div class="form-control">
				<label for="server-name" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_server_name()}</span>
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
					<span class="label-text text-sm">{m.hostserver_instance()}</span>
				</label>
				<select
					id="instance"
					class="select select-bordered w-full"
					bind:value={selectedInstanceId}
				>
					<option value="" disabled>{m.hostserver_select_instance()}</option>
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
					<span class="label-text text-sm">{m.hostserver_gamemode()}</span>
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
			<span class="label-text text-sm label">{m.hostserver_public_hint()}</span>
			<div class="form-control flex items-center justify-between">
				<label for="public" class="label">
					<span class="label-text text-sm">{m.hostserver_public()}</span>
				</label>
				<input id="public" class="toggle" type="checkbox" bind:checked={selectedPublic} />
				<label for="password" class="label">
					<span class="label-text text-sm">{m.hostserver_password()}</span>
				</label>
				<input
					id="password"
					type="text"
					placeholder={m.hostserver_leave_empty_password()}
					maxlength="60"
					class="input input-bordered"
					bind:value={selectedPassword}
				/>
			</div>
			<div class="form-control">
				<label for="server-tags" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_tags_separated()}</span>
					<span class="label-text-alt text-xs">{m.hostserver_tags_optional()}</span>
				</label>
				<input
					id="server-tags"
					type="text"
					maxlength="30"
					class="input input-bordered w-full"
					bind:value={selectedTags}
				/>
			</div>
		</div>

		<!-- Advanced tab -->
		<div
			class={[
				"space-y-4 row-start-1 col-start-1",
				selectedTab !== "advanced" && "invisible pointer-events-none",
			]}
		>
			<div class="form-control">
				<label for="server-max-players" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_max_players()}</span>
				</label>
				<input
					id="server-max-players"
					type="number"
					class="input input-bordered w-full user-invalid:validator"
					required
					min="1"
					max="30"
					placeholder={m.hostserver_players_range()}
					bind:value={selectedMaxPlayers}
				/>
			</div>
			<div class="form-control">
				<label for="server-min-players" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_min_players()}</span>
				</label>
				<input
					id="server-min-players"
					type="number"
					class="input input-bordered w-full user-invalid:validator"
					required
					min="1"
					max="30"
					placeholder={m.hostserver_players_range_min()}
					bind:value={selectedMinPlayers}
				/>
			</div>
			<div class="form-control">
				<label for="server-max-spectators" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_max_spectators()}</span>
				</label>
				<input
					id="server-max-spectators"
					type="number"
					class="input input-bordered w-full user-invalid:validator"
					required
					min="1"
					max="30"
					placeholder={m.hostserver_spectators_range()}
					bind:value={selectedMaxSpectators}
				/>
			</div>
			<div class="form-control">
				<label for="server-port" class="label py-0.5">
					<span class="label-text text-sm">{m.hostserver_port()}</span>
				</label>
				<input
					id="server-port"
					type="number"
					class="input input-bordered w-full user-invalid:validator"
					required
					min="50000"
					max="65000"
					placeholder={m.hostserver_port_range()}
					bind:value={selectedPort}
				/>
			</div>
		</div>
	</div>

	{#snippet actions()}
		<div class="flex items-center justify-end w-full">
			<div class="flex gap-2">
				<button class="btn btn-ghost" onclick={() => (open = false)}
					>{m.common_cancel()}</button
				>
				<button class="btn btn-accent" disabled={!valid} onclick={handleCreate}>
					{m.common_create()}
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
