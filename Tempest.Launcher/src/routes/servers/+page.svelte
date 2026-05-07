<script lang="ts">
	import { RefreshCw, Search, Server, ServerCrash } from "@lucide/svelte";
	import { goto } from "$app/navigation";
	import Header from "$lib/components/ui/Header.svelte";
	import { moveToLobby } from "$lib/core/lobby";
	import { m } from "$lib/paraglide/messages";
	import { createServersQuery } from "$lib/queries/servers";
	import { CountryCode } from "$lib/rpc";
	import { lobbyHost, lobbyPassword } from "$lib/stores/lobby";
	import { hostServerWizardOpen } from "$lib/stores/ui";

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
		return [...code.toUpperCase()]
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
		return m.serverlist_error_fetching();
	}

	function refreshServers() {
		searchQuery = "";
		serversQuery.refetch();
	}
</script>

<svelte:head>
	<title>{m.serverlist_title()}</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	<Header title={m.serverlist_title()}>
		{#snippet icon()}
			<Server size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
			<label class="input input-bordered">
				<Search size={16} class="opacity-50" />
				<input
					type="text"
					placeholder={m.serverlist_search_placeholder()}
					class="grow"
					bind:value={searchQuery}
				/>
			</label>
			<button
				class="btn btn-accent"
				onclick={() => {
					hostServerWizardOpen.set(true);
				}}
			>
				{m.serverlist_host_server()}
			</button>
			<button
				class="btn btn-ghost btn-square"
				onclick={refreshServers}
				aria-label={m.serverlist_refresh_aria_label()}
			>
				<RefreshCw size={16} />
			</button>
		{/snippet}
		{#snippet subtitle()}
			<span
				>{serverCount}
				{serverCount === 1 ? m.serverlist_server() : m.serverlist_servers()}</span
			>
			{#if isFetching}
				<span class="inline-flex items-center gap-2">
					<span class="loading loading-spinner loading-xs"></span>
					{m.serverlist_refreshing()}
				</span>
			{/if}
		{/snippet}
	</Header>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				{#if isLoading}
					<div role="alert" class="alert alert-info">
						<span class="loading loading-spinner loading-sm"></span>
						<span>{m.serverlist_loading()}</span>
					</div>
				{:else if error}
					<div role="alert" class="alert alert-error">
						<ServerCrash />
						<span>{formatQueryError(serversQuery.error)}</span>
					</div>
				{:else if servers.length === 0}
					<div role="alert" class="alert alert-info">
						<span>{m.serverlist_no_servers()}</span>
					</div>
				{:else if filteredServers.length === 0}
					<div role="alert" class="alert alert-info">
						<span>{m.serverlist_no_servers_matching()}</span>
					</div>
				{:else}
					<div class="overflow-x-auto">
						<table class="table table-zebra">
							<thead>
								<tr>
									<th>{m.serverlist_name()}</th>
									<th>{m.serverlist_gamemode()}</th>
									<th>{m.serverlist_map()}</th>
									<th>{m.serverlist_players()}</th>
									<th>{m.serverlist_bots()}</th>
									<th>{m.serverlist_spectators()}</th>
									<th>{m.serverlist_version()}</th>
									<th>{m.serverlist_password()}</th>
									<th>{m.serverlist_tags()}</th>
									<th>{m.serverlist_country()}</th>
								</tr>
							</thead>
							<tbody>
								{#each filteredServers as server (server.id)}
									<tr
										onclick={() => {
											moveToLobby(`http://${server.ip}:${server.lobbyPort}`);
										}}
									>
										<td>{server.name}</td>
										<td>{server.game}</td>
										<td>{server.map || m.common_na()}</td>
										<td>
											<span
												class={[
													"badge",
													server.players >= server.maxPlayers ?
														"badge-error"
													:	"badge-success",
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
													:	"badge-success",
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
													server.hasPassword ? "badge-error" : (
														"badge-success"
													),
												]}
											>
												{server.hasPassword ?
													m.common_yes()
												:	m.common_no()}
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
