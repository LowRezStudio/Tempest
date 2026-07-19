<script lang="ts">
	import {
		RefreshCw,
		Search,
		Server,
		ServerCrash,
		ServerOff,
	} from "@lucide/svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import ServerRow from "$lib/components/server-list/ServerRow.svelte";
	import ServerDetailsDialog from "$lib/components/server-list/ServerDetailsDialog.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createServersQuery, createLanServersQuery } from "$lib/queries/servers";
	import { hostServerWizardOpen, joinServerWizardOpen } from "$lib/stores/ui.svelte";
	import type { ServerListing } from "$lib/rpc";
	import { untrack } from "svelte";

	let searchQuery = $state("");

	let activeTab = $state<"internet" | "lan">("internet");
	const tabs = [
		{ name: m.serverlist_tab_internet(), value: "internet" as const },
		{ name: m.serverlist_tab_lan(), value: "lan" as const },
	];

	const serversQuery = createServersQuery();
	const lanQuery = createLanServersQuery();

	$effect(() => {
		if (activeTab === "lan") {
			untrack(() => lanQuery.refetch());
		}
	});

	let servers = $derived(
		activeTab === "internet" ? (serversQuery.data ?? []) : (lanQuery.data ?? []),
	);
	let error = $derived(
		activeTab === "internet" ? (serversQuery.isError ?? false) : (lanQuery.isError ?? false),
	);
	let isLoading = $derived(
		activeTab === "internet" ? (serversQuery.isLoading ?? false) : (lanQuery.isLoading ?? false),
	);
	let isFetching = $derived(
		activeTab === "internet" ? (serversQuery.isFetching ?? false) : (lanQuery.isFetching ?? false),
	);

	const filteredServers = $derived(
		servers.filter(
			(server) =>
				server.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.gamemode.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.map?.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.version.toLowerCase().includes(searchQuery.toLowerCase()) ||
				server.tags.some((tag) =>
					tag.toLowerCase().includes(searchQuery.toLowerCase()),
				),
		),
	);

	const serverCount = $derived(servers.length);

	let selectedServer = $state<ServerListing | null>(null);
	let detailsOpen = $state(false);

	function openDetails(server: ServerListing) {
		selectedServer = server;
		detailsOpen = true;
	}

	function formatQueryError(err: unknown) {
		if (typeof err === "string" && err) return err;
		if (err instanceof Error && err.message) return err.message;
		return m.serverlist_error_fetching();
	}

	function refreshServers() {
		searchQuery = "";
		if (activeTab === "internet") {
			serversQuery.refetch();
		} else {
			lanQuery.refetch();
		}
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header
		title={m.serverlist_title()}
		{tabs}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
	>
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
					joinServerWizardOpen.value = true;
				}}
			>
				{m.join_server_title()}
			</button>
			<button
				class="btn btn-accent"
				onclick={() => {
					hostServerWizardOpen.value = true;
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
				{m.serverlist_servers({ count: serverCount })}</span
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
					<div class="flex flex-col items-center justify-center h-64 gap-4">
						<span class="loading loading-spinner loading-lg text-base-content/50"
						></span>
						<p class="text-lg text-base-content/50">{m.serverlist_loading()}</p>
					</div>
				{:else if error}
					<div role="alert" class="alert alert-error">
						<ServerCrash />
						<span>{formatQueryError(activeTab === "internet" ? serversQuery.error : lanQuery.error)}</span>
					</div>
				{:else if servers.length === 0}
					<div class="flex flex-col items-center justify-center h-64 gap-4">
						<ServerOff size={48} class="opacity-30" />
						<p class="text-lg text-base-content/50">{m.serverlist_no_servers()}</p>
						<p class="text-sm text-base-content/40">
							{m.serverlist_no_servers_hint()}
						</p>
					</div>
				{:else if filteredServers.length === 0}
					<div class="flex flex-col items-center justify-center h-64 gap-4">
						<Search size={48} class="opacity-30" />
						<p class="text-lg text-base-content/50">
							{m.serverlist_no_servers_matching()}
						</p>
						<p class="text-sm text-base-content/40">
							{m.serverlist_no_servers_matching_hint()}
						</p>
					</div>
				{:else}
					<div class="flex flex-col gap-3">
						{#each filteredServers as server (server.id)}
							<ServerRow
								{server}
								showCountry={activeTab === "internet"}
								onclick={openDetails}
							/>
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>

<ServerDetailsDialog
	server={selectedServer}
	showCountry={activeTab === "internet"}
	bind:open={detailsOpen}
/>
