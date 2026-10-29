<script lang="ts">
	import {
		ArrowDownAZ,
		ArrowDownWideNarrow,
		AlertTriangle,
		Check,
		ChevronDown,
		RefreshCw,
		Search,
		Server,
		ServerCrash,
		ServerOff,
		Swords,
		Users,
		X,
	} from "@lucide/svelte";
	import { DropdownMenu } from "bits-ui";
	import { untrack } from "svelte";
	import ServerDetailsDialog from "$lib/components/server-list/ServerDetailsDialog.svelte";
	import ServerRow from "$lib/components/server-list/ServerRow.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createServersQuery, createLanServersQuery } from "$lib/queries/servers";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";
	import { dismissExperimentalWarning } from "$lib/stores/settings.svelte";
	import { hostServerWizardOpen, joinServerWizardOpen } from "$lib/stores/ui.svelte";
	import type { ServerListing } from "$lib/rpc";

	let searchQuery = $state("");

	const publicServersEnabled = $derived(isFlagEnabled("public-servers"));

	let activeTab = $state<"internet" | "lan">(
		untrack(() => (isFlagEnabled("public-servers") ? "internet" : "lan")),
	);
	const tabs = $derived([
		...(publicServersEnabled
			? [{ name: m.serverlist_tab_internet(), value: "internet" as const }]
			: []),
		{ name: m.serverlist_tab_lan(), value: "lan" as const },
	]);

	const serversQuery = createServersQuery(() => publicServersEnabled);
	const lanQuery = createLanServersQuery();

	$effect(() => {
		if (activeTab === "lan") {
			untrack(() => lanQuery.refetch());
		}
		if (!publicServersEnabled && activeTab === "internet") {
			activeTab = "lan";
		}
	});

	let servers = $derived(
		activeTab === "internet" ? (serversQuery.data ?? []) : (lanQuery.data ?? []),
	);
	let error = $derived(
		activeTab === "internet" ? (serversQuery.isError ?? false) : (lanQuery.isError ?? false),
	);
	let isLoading = $derived(
		activeTab === "internet"
			? (serversQuery.isLoading ?? false)
			: (lanQuery.isLoading ?? false),
	);
	let isFetching = $derived(
		activeTab === "internet"
			? (serversQuery.isFetching ?? false)
			: (lanQuery.isFetching ?? false),
	);

	type ServerArrangement = "players" | "name" | "gamemode";
	let arrangement = $state<ServerArrangement>("players");
	const arrangementOptions = [
		{ value: "players" as const, Icon: ArrowDownWideNarrow, label: m.serverlist_players },
		{ value: "name" as const, Icon: ArrowDownAZ, label: m.common_name },
		{ value: "gamemode" as const, Icon: Swords, label: m.serverlist_gamemode },
	];
	const currentArrangement = $derived(
		arrangementOptions.find((option) => option.value === arrangement) ?? arrangementOptions[0]!,
	);
	const CurrentArrangementIcon = $derived(currentArrangement.Icon);

	const filteredServers = $derived(
		[...servers]
			.sort((a, b) => {
				switch (arrangement) {
					case "name": {
						return a.name.localeCompare(b.name);
					}
					case "gamemode": {
						return a.gamemode.localeCompare(b.gamemode) || a.name.localeCompare(b.name);
					}
					default: {
						return b.players - a.players || a.name.localeCompare(b.name);
					}
				}
			})
			.filter(
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

<div class="flex h-full flex-col">
	<Header
		title={m.serverlist_title()}
		tabs={tabs.length > 1 ? tabs : undefined}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
	>
		{#snippet icon()}
			<Server size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
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

	<div class="flex flex-1 flex-col overflow-hidden">
		{#if !dismissExperimentalWarning.value}
			<div role="alert" class="alert alert-warning mx-4 mt-4">
				<AlertTriangle size={20} />
				<span>{m.serverlist_experimental_warning()}</span>
				<button
					class="btn btn-ghost btn-sm"
					onclick={() => (dismissExperimentalWarning.value = true)}
					aria-label="Dismiss warning"
				>
					<X size={16} />
				</button>
			</div>
		{/if}
		<div class="@container min-h-0 flex-1 overflow-y-auto">
			<div class="mx-auto w-full max-w-6xl px-4 py-6">
				<div class="mb-4 flex gap-2">
					<label class="input input-sm min-w-0 flex-1">
						<Search size={14} class="opacity-50" />
						<input
							type="text"
							placeholder={m.serverlist_search_placeholder()}
							class="grow"
							bind:value={searchQuery}
						/>
					</label>
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							class="btn btn-sm shrink-0 flex-nowrap gap-1.5 font-normal"
							aria-label={m.library_arrange()}
						>
							<CurrentArrangementIcon size={14} class="opacity-70" />
							{currentArrangement.label()}
							<ChevronDown size={14} class="opacity-60" />
						</DropdownMenu.Trigger>
						<DropdownMenu.Portal>
							<DropdownMenu.Content
								data-popover-menu
								class="bg-base-200 border-base-300 rounded-box popover-animate z-50 flex w-56 flex-col gap-0.5 border p-1.5 shadow-xl outline-none"
								align="end"
								sideOffset={4}
							>
								<span
									class="text-base-content/50 px-2 pt-1 pb-1.5 text-xs font-semibold"
								>
									{m.library_arrange()}
								</span>
								<DropdownMenu.RadioGroup
									bind:value={
										() => arrangement,
										(value) => (arrangement = value as ServerArrangement)
									}
								>
									{#each arrangementOptions as option (option.value)}
										{@const Icon = option.Icon}
										<DropdownMenu.RadioItem
											value={option.value}
											class="rounded-field data-highlighted:bg-base-300 flex cursor-pointer items-center gap-2.5 px-2 py-1.5 text-sm outline-none"
										>
											{#snippet children({ checked })}
												<Icon size={14} class="shrink-0 opacity-70" />
												<span class:font-semibold={checked}
													>{option.label()}</span
												>
												{#if checked}
													<Check size={14} class="text-primary ml-auto" />
												{/if}
											{/snippet}
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
							</DropdownMenu.Content>
						</DropdownMenu.Portal>
					</DropdownMenu.Root>
				</div>
				<div class="grid items-start gap-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]">
					<div class="min-w-0">
						{#if isLoading}
							<div class="flex h-64 flex-col items-center justify-center gap-4">
								<span
									class="loading loading-spinner loading-lg text-base-content/50"
								></span>
								<p class="text-base-content/50 text-lg">{m.serverlist_loading()}</p>
							</div>
						{:else if error}
							<div role="alert" class="alert alert-error">
								<ServerCrash />
								<span
									>{formatQueryError(
										activeTab === "internet"
											? serversQuery.error
											: lanQuery.error,
									)}</span
								>
							</div>
						{:else if servers.length === 0}
							<div class="flex h-64 flex-col items-center justify-center gap-4">
								<ServerOff size={48} class="opacity-30" />
								<p class="text-base-content/50 text-lg">
									{m.serverlist_no_servers()}
								</p>
								<p class="text-base-content/40 text-sm">
									{m.serverlist_no_servers_hint()}
								</p>
							</div>
						{:else if filteredServers.length === 0}
							<div class="flex h-64 flex-col items-center justify-center gap-4">
								<Search size={48} class="opacity-30" />
								<p class="text-base-content/50 text-lg">
									{m.serverlist_no_servers_matching()}
								</p>
								<p class="text-base-content/40 text-sm">
									{m.serverlist_no_servers_matching_hint()}
								</p>
							</div>
						{:else}
							<div class="bg-base-200/40 border-base-300 rounded-box border p-4">
								<div class="flex flex-col gap-3">
									{#each filteredServers as server (server.id)}
										<ServerRow
											{server}
											showCountry={activeTab === "internet"}
											onclick={openDetails}
										/>
									{/each}
								</div>
							</div>
						{/if}
					</div>
					<aside class="order-first @3xl:sticky @3xl:top-6 @3xl:order-last">
						<section class="bg-base-200/70 border-base-300 rounded-box border p-4">
							<h2 class="text-base font-semibold">Server Actions</h2>
							<p class="text-base-content/60 mt-1 mb-4 text-sm">
								Servers on your LAN or VPN will show in the server list.
								<br />
								Or join a server with its IP and port forwarding.
							</p>
							<div class="flex gap-2 @3xl:flex-col">
								<button
									type="button"
									class="bg-base-100 border-base-300 hover:border-primary/60 rounded-field flex flex-1 cursor-pointer items-start gap-3 border p-3 text-left text-sm transition-colors"
									onclick={() => (joinServerWizardOpen.value = true)}
								>
									<Users size={16} class="text-primary mt-0.5 shrink-0" />
									<span class="font-semibold">{m.join_server_title()}</span>
								</button>
								<button
									type="button"
									class="bg-base-100 border-base-300 hover:border-primary/60 rounded-field flex flex-1 cursor-pointer items-start gap-3 border p-3 text-left text-sm transition-colors"
									onclick={() => (hostServerWizardOpen.value = true)}
								>
									<Server size={16} class="text-primary mt-0.5 shrink-0" />
									<span class="font-semibold">{m.serverlist_host_server()}</span>
								</button>
							</div>
						</section>
					</aside>
				</div>
			</div>
		</div>
	</div>
</div>

<ServerDetailsDialog
	server={selectedServer}
	showCountry={activeTab === "internet"}
	bind:open={detailsOpen}
/>
