<script lang="ts">
	import { CountryCode } from "$lib/rpc";
	import { createServersQuery } from "$lib/queries/servers";
	import { RefreshCw, Server, ServerCrash, Search } from "@lucide/svelte";

	let searchQuery = $state("");

	const serversQuery = createServersQuery();

	let servers = $derived(serversQuery.data ?? []);
	let error = $derived(serversQuery.isError ?? false);
	let isLoading = $derived(serversQuery.isLoading ?? false);
	let isFetching = $derived(serversQuery.isFetching ?? false);

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

	const serverCount = $derived(servers.length);

	function toFlagEmoji(code: string) {
		if (!code || code.length !== 2) {
			return "";
		}
		return code
			.toUpperCase()
			.split("")
			.map((char) => String.fromCodePoint(127397 + char.charCodeAt(0)))
			.join("");
	}

	function formatCountryLabel(code: string) {
		const flag = toFlagEmoji(code);
		return flag ? `${flag} ${code}` : code;
	}

	function formatQueryError(error: unknown) {
		if (error instanceof Error && error.message) {
			return error.message;
		}
		return "Error while fetching the server list.";
	}

	function refreshServers() {
		searchQuery = "";
		serversQuery.refetch();
	}
</script>

<svelte:head>
	<title>Server list</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
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
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<span>{serverCount} {serverCount === 1 ? "server" : "servers"}</span>
							{#if isFetching}
								<span class="inline-flex items-center gap-2">
									<span class="loading loading-spinner loading-xs"></span>
									Refreshing
								</span>
							{/if}
						</div>
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
					<button
						class="btn btn-ghost btn-square"
						onclick={refreshServers}
						aria-label="Refresh servers"
					>
						<RefreshCw size={16} />
					</button>
				</div>
			</div>
		</div>
	</div>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				{#if isLoading}
					<div role="alert" class="alert alert-info">
						<span class="loading loading-spinner loading-sm"></span>
						<span>Loading servers…</span>
					</div>
				{:else if error}
					<div role="alert" class="alert alert-error">
						<ServerCrash />
						<span>{formatQueryError(serversQuery.error)}</span>
					</div>
				{:else if servers.length === 0}
					<div role="alert" class="alert alert-info">
						<span>No servers available.</span>
					</div>
				{:else if filteredServers.length === 0}
					<div role="alert" class="alert alert-info">
						<span>No servers matching search.</span>
					</div>
				{:else}
					<div class="overflow-x-auto">
						<table class="table table-zebra">
							<thead>
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
										onclick={() => {
											//TODO
										}}
									>
										<td>{server.name}</td>
										<td>{server.game}</td>
										<td>{server.map || "N/A"}</td>
										<td>
											<span
												class={[
													"badge",
													server.players >= server.maxPlayers ?
														"badge-error"
													: 	"badge-success",
												]}
											>
												{server.players}/{server.maxPlayers}
											</span>
										</td>
										<td>{server.bots}</td>
										<td>
											<span
												class={[
													"badge",
													server.spectators >= server.maxSpectators ?
														"badge-error"
													: 	"badge-success",
												]}
											>
												{server.spectators}/{server.maxSpectators}
											</span>
										</td>
										<td>{server.version}</td>
										<td>
											<span
												class={[
													"badge",
													server.hasPassword ? "badge-error" : "badge-success",
												]}
											>
												{server.hasPassword ? "Yes" : "No"}
											</span>
										</td>
										<td>{server.tags.join(", ")}</td>
										<td>{formatCountryLabel(CountryCode[server.country])}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>
