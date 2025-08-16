<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Input from "$lib/components/ui/Input.svelte";
	import {
		ensureLoaded,
		list as serverList,
		refresh as refreshServers,
		type SearchFilters,
		searchFilters,
	} from "$lib/state/servers.svelte";
	import { Box, Map, RefreshCcw, Server, ServerOff, Users } from "@lucide/svelte";
	import { onMount } from "svelte";

	let refreshing = $state(false);

	// const regionNames: Record<SearchFilters["region"], string> = {
	// 	"any": "Any",
	// 	"as": "Asia",
	// 	"eu": "Europe",
	// 	"me": "Middle East / North Africa",
	// 	"na": "North America",
	// 	"oc": "Oceania",
	// 	"sa": "South Africa"
	// };

	// const regionItems = Object.entries(regionNames)
	// 	.map(([value, label]) => ({ value, label }));

	const filtered = $derived(
		serverList
			.filter(s => s.name.toUpperCase().includes(searchFilters.name.toUpperCase())),
	);

	async function refresh() {
		refreshing = true;
		try {
			await refreshServers();
		} finally {
			refreshing = false;
		}
	}

	onMount(() => {
		ensureLoaded();
	});
</script>

<section class="w-full h-full overflow-auto">
	<div class="mx-auto max-w-6xl px-6 py-6">
		<div class="flex items-center justify-between gap-4">
			<div>
				<h1 class="text-2xl font-semibold tracking-tight">Servers</h1>
				<p class="mt-1 text-sm text-background-300">Browse and join community and official servers.</p>
			</div>
			<Button kind="accented" icon={RefreshCcw} disabled={refreshing} onclick={refresh} aria-label="Refresh servers">
				{refreshing ? "Refreshing" : "Refresh"}
			</Button>
		</div>

		<div class="mt-6 rounded-2xl border border-background-800 bg-background-950/60 p-4">
			<div class="grid grid-cols-1 gap-3 md:grid-cols-[1fr,auto,auto]">
				<div class="flex flex-col gap-1 min-w-0">
					<label class="text-sm" for="server-name">Server Name</label>
					<Input id="server-name" placeholder="Search by name" bind:value={searchFilters.name} class="w-full" />
				</div>
			</div>
		</div>

		{#if filtered.length === 0}
			<div
				class="mt-8 grid place-items-center rounded-2xl border border-background-800 bg-gradient-to-b from-background-950 to-background-900/60 p-10"
			>
				<div class="text-center max-w-xl">
					<div class="mx-auto grid place-items-center size-14 rounded-full bg-background-800 text-background-300">
						<ServerOff class="size-7" />
					</div>
					<p class="mt-4 text-background-200 text-base font-medium">No servers match your filters.</p>
				</div>
			</div>
		{:else}
			<div class="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
				{#each filtered as s (s.id)}
					<div
						class="group rounded-2xl border border-background-800 bg-background-950 p-4 transition-colors hover:bg-background-900"
					>
						<div class="flex items-start gap-4">
							<div
								class="grid place-items-center size-12 rounded-xl bg-background-900 border-2 border-background-700 text-primary-300 shadow-inner"
							>
								<Server class="size-6" />
							</div>
							<div class="min-w-0 flex-auto">
								<div class="flex items-center gap-2">
									<p class="font-semibold tracking-tight truncate">{s.name}</p>
								</div>
								<div class="mt-1 flex flex-wrap items-center gap-2 text-xs text-background-300">
									<!-- <span class="inline-flex items-center gap-1">
										<Globe class="size-3.5 text-secondary-300" /> {s.region}
									</span> -->
									<span class="inline-flex items-center gap-1">
										<Box class="size-3.5 text-secondary-300" /> {s.version}
									</span>
									{#if s.map.length != 0}
										<span class="inline-flex items-center gap-1">
											<Map class="size-3.5 text-secondary-300" /> {s.map}
										</span>
									{/if}
									<span class="inline-flex items-center gap-1">
										<Users class="size-3.5 text-secondary-300" /> {s.players} / {s.maxPlayers}
									</span>
								</div>
							</div>
						</div>
						<div class="mt-4 flex items-center justify-between gap-3">
							<div class="text-sm text-background-300">{s.joinable ? "Joinable" : "Not joinable"}</div>
							<Button
								disabled={!s.joinable || s.players >= s.maxPlayers}
								kind={!s.joinable || s.players >= s.maxPlayers ? undefined : "accented"}
							>
								{s.players >= s.maxPlayers ? "Full" : s.joinable ? "Join" : "Unavailable"}
							</Button>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</section>

<style>
	@reference "../../lib/styles/global.css";
</style>
