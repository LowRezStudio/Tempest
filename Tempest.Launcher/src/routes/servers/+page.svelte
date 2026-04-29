<script lang="ts">
	import { RefreshCw, Search, Server, ServerCrash, TriangleAlert } from "@lucide/svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { moveToLobby } from "$lib/core/lobby";
	import maps from "$lib/data/maps.json";
	import { m } from "$lib/paraglide/messages";
	import { createServersQuery } from "$lib/queries/servers";
	import { CountryCode, ServerListing } from "$lib/rpc";
	import { instanceMap } from "$lib/stores/instance";
	import { hostServerWizardOpen, joinServerWizardOpen } from "$lib/stores/ui";
	import { getMapsForVersion } from "$lib/utils/versions";

	let searchQuery = $state("");

	const serversQuery = createServersQuery();

	let servers = $derived(serversQuery.data ?? []);
	let error = $derived(serversQuery.isError ?? false);
	let isLoading = $derived(serversQuery.isLoading ?? false);
	let isFetching = $derived(serversQuery.isFetching ?? false);

	const filteredServers = $derived(
		servers
			.filter(
				(server) =>
					server.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
					server.game.toLowerCase().includes(searchQuery.toLowerCase()) ||
					server.map?.toLowerCase().includes(searchQuery.toLowerCase()) ||
					server.version.toLowerCase().includes(searchQuery.toLowerCase()) ||
					server.tags.some((tag) =>
						tag.toLowerCase().includes(searchQuery.toLowerCase()),
					),
			)
			.map((s) => ({ ...s, canJoin: canJoinServer(s) })),
	);
	function canJoinServer(server: ServerListing) {
		//TODO check mods too
		return Object.values($instanceMap).some((i) => i.version === server.version);
	}
	function findMapName(server: ServerListing) {
		if (!server.map && !server.mapId) return m.common_na();
		if (server.map) return server.map;
		const map = getMapsForVersion(server.version).find((m) => m.id === server.mapId);
		if (!map) return server.mapId;
		return map.displayName;
	}
	function findGamemodeName(server: ServerListing) {
		if (server.game.startsWith("TempestMp.")) {
			return server.game.substring(server.game.indexOf(".") + 1);
		}
		return server.game;
	}

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
					joinServerWizardOpen.set(true);
				}}
			>
				Join server
			</button>
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
											if (!server.canJoin) return;
											moveToLobby(`http://${server.ip}:${server.lobbyPort}`);
										}}
										class={!server.canJoin ? "text-base-content/70" : ""}
									>
										<td>
											<span class="flex justify-between">
												{server.name}
												{#if !server.canJoin}
													<span
														class="group"
														style="anchor-scope: --cannot-join;"
													>
														<TriangleAlert
															size={20}
															style="anchor-name: --cannot-join;"
														></TriangleAlert>
														<div
															class="tooltip tooltip-right tooltip-open pointer-events-none fixed h-0 w-0 opacity-0 transition-opacity duration-100 group-hover:opacity-100"
															style="position-anchor: --cannot-join; top: anchor(center); left: anchor(center);"
															data-tip={`Requires version ${server.version}`}
															aria-hidden="true"
														></div>
													</span>
												{/if}
											</span>
										</td>
										<td>{findGamemodeName(server)}</td>
										<td>{findMapName(server)}</td>
										<td>
											<span
												class={[
													"badge",
													!server.canJoin ? "badge-neutral"
													: server.players >= server.maxPlayers ?
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
													!server.canJoin ? "badge-neutral"
													: server.spectators >= server.maxSpectators ?
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
													!server.canJoin ? "badge-neutral"
													: server.hasPassword ? "badge-error"
													: "badge-success",
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
