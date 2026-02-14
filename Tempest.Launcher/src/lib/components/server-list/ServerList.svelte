<script lang="ts">
	import { CountryCode, ServerListing } from "$lib/rpc";
	import { ServerListClient } from "$lib/rpc/server_list/server_list_service.client";
	import { Server, ServerCrash, Search } from "@lucide/svelte";
	import { GrpcWebFetchTransport } from "@protobuf-ts/grpcweb-transport";
	import "flag-icons/css/flag-icons.min.css";
	interface Props {}

	let {}: Props = $props();

	//temporarily here
	const transport = new GrpcWebFetchTransport({
		baseUrl: "http://localhost:5197",
	});
	const client = new ServerListClient(transport);

	let servers = $state<ServerListing[]>([]);
	let searchQuery = $state("");
	let error = $state<boolean>(false);

	const filteredServers = $derived(
		servers.filter(
			(server) =>
				server.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.game.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.map?.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.version.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.tags.some((tag) => tag.toLowerCase().includes(searchQuery.toLowerCase())),
		),
	);
	async function fetchServers() {
		const server_list = [];
		try {
			const call = client.getServers({});
			for await (const resp of call.responses) {
				server_list.push(resp);
			}
			error = false;
		} catch (e) {
			console.log(e);
			error = true;
		}

		servers = server_list;
	}
	$effect(() => {
		fetchServers();

		const interval = setInterval(() => {
			fetchServers();
		}, 2000);

		return () => clearInterval(interval);
	});
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<div class="bg-base-200">
		<div class="px-4 py-3">
			<div class="flex items-center justify-between">
				<div class="flex items-center gap-3">
					<div
						class="w-16 h-16 rounded-xl bg-base-300 flex items-center justify-center shrink-0"
					>
						<Server size={32} class="opacity-60" />
					</div>
					<div>
						<h1 class="text-2xl font-bold mb-1">Server List</h1>
					</div>
				</div>

				<div class="flex items-center gap-2">
					<label class="input input-bordered">
						<Search size={16} class="opacity-50" />
						<input
							type="text"
							placeholder="Search"
							class="grow"
							bind:value={searchQuery}
						/>
					</label>
					<button
						class="btn btn-accent"
						onclick={() => {
							//TODO
						}}
					>
						Host server
					</button>
				</div>
			</div>
		</div>
	</div>
	<div class="flex w-full p-9 overflow-auto">
		<table class="w-full">
			<thead class="text-left">
				<tr>
					<th>Name</th>
					<th>Gamemode</th>
					<th>Map</th>
					<th>Players</th>
					<th>Bots</th>
					<th>Spectators</th>
					<th>Version</th>
					<th>Password</th>
					<th>Tags</th>
					<th>Country</th>
				</tr>
			</thead>
			<tbody>
				{#each filteredServers as server (server.id)}
					<tr
						class="hover:bg-base-300 border-b-black border-b-1 h-9 cursor-pointer"
						onclick={() => {
							//TODO
						}}
					>
						<td
							><p
								class="max-w-50 w-50 overflow-hidden whitespace-nowrap box-content mr-3"
							>
								{server.name}
							</p></td
						>
						<td>{server.game}</td>
						<td class="max-w-50 w-50">{server.map || "N/A"}</td>
						<td
							class={[
								server.players >= server.maxPlayers ?
									"text-red-300"
								:	"text-lime-200",
							]}>{server.players}/{server.maxPlayers}</td
						>
						<td>{server.bots}</td>
						<td
							class={[
								server.spectators >= server.maxSpectators ?
									"text-red-300"
								:	"text-lime-200",
							]}>{server.spectators}/{server.maxSpectators}</td
						>
						<td>{server.version}</td>
						<td class={[server.hasPassword ? "text-red-300" : "text-lime-200"]}
							>{server.hasPassword ? "Yes" : "No"}</td
						>
						<td>{server.tags.join(",")}</td>
						<td>
							<span class={"fi fi-" + CountryCode[server.country].toLowerCase()}
							></span>
							{CountryCode[server.country]}</td
						>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	{#if error}
		<div class="flex h-full w-full justify-center gap-3 text-lg">
			<ServerCrash />
			<p>Error while fetching the server list!</p>
			<ServerCrash />
		</div>
	{/if}
	{#if !error && servers.length == 0}
		<div class="flex h-full w-full justify-center text-lg">
			<p>No servers</p>
		</div>
	{/if}
	{#if !error && filteredServers.length == 0 && servers.length > 0}
		<div class="flex h-full w-full justify-center text-lg">
			<p>No servers matching search</p>
		</div>
	{/if}
</div>
