<script lang="ts">
	import {
		Gamepad2,
		Globe,
		Lock,
		MapPin,
		Swords,
		Tag,
		TriangleAlert,
		Users,
	} from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { moveToLobby } from "$lib/core/lobby.svelte";
	import { m } from "$lib/paraglide/messages";
	import { CountryCode, type ServerListing } from "$lib/rpc";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { getMapsForVersion } from "$lib/utils/versions";

	interface Props {
		server: ServerListing | null;
		showCountry?: boolean;
		open?: boolean;
	}

	let { server, showCountry = false, open = $bindable(false) }: Props = $props();

	const canJoin = $derived(
		server
			? Object.values(instanceMap.value).some((i) => i && i.version === server.version)
			: false,
	);



	const countryLabel = $derived(server ? formatCountry(server.country) : "");

	function findMapName(s: ServerListing): string {
		if (!s.map && !s.mapId) return m.common_na();
		if (s.map) return s.map;
		const mapEntry = getMapsForVersion(s.version).find((m) => m.id === s.mapId);
		if (!mapEntry) return s.mapId ?? m.common_na();
		return mapEntry.displayName;
	}

	function findGamemodeName(s: ServerListing): string {
		if (s.gamemode.startsWith("TempestMp.")) {
			return s.gamemode.slice(s.gamemode.indexOf(".") + 1);
		}
		return s.gamemode;
	}

	function toFlagEmoji(code: string): string {
		if (!code || code.length !== 2) return "";
		return [...code.toUpperCase()]
			.map((char) => String.fromCodePoint(127397 + char.charCodeAt(0)))
			.join("");
	}

	function formatCountry(code: CountryCode): string {
		const str = CountryCode[code];
		if (!str || str === "COUNTRY_CODE_UNSPECIFIED") return "";
		const flag = toFlagEmoji(str);
		return flag ? `${flag} ${str}` : str;
	}

	function handleJoin() {
		if (!server || !canJoin) return;
		moveToLobby(`http://${server.ip}:${server.lobbyPort}`);
		open = false;
	}
</script>

{#if server}
	<Modal bind:open title={server.name} class="max-w-md" onsubmit={handleJoin}>
		<div class="flex flex-col gap-4">
			<!-- Quick facts -->
			<div class="flex flex-wrap items-center gap-1">
				<span class="badge badge-soft badge-primary badge-sm gap-1">
					<Swords size={12} />
					{findGamemodeName(server)}
				</span>
				<span class="badge badge-ghost badge-sm gap-1">
					<MapPin size={12} />
					{findMapName(server)}
				</span>
				<span class="badge badge-ghost badge-sm gap-1">
					<Gamepad2 size={12} />
					{server.version}
				</span>
				{#if server.hasPassword}
					<span class="badge badge-soft badge-warning badge-sm gap-1">
						<Lock size={12} />
						{m.common_password()}
					</span>
				{/if}
			</div>

			<!-- Players capacity -->
			<div class="stats stats-horizontal shadow-sm bg-base-200 w-full">
				<div class="stat">
					<div class="stat-figure text-base-content/40">
						<Users size={28} />
					</div>
					<div class="stat-title">{m.serverlist_players()}</div>
					<div class="stat-value text-2xl">
						{server.players}
						<span class="text-base-content/40 text-base font-normal"
							>/ {server.maxPlayers}</span
						>
					</div>
					<div class="stat-desc">
						{#if server.spectators > 0}
							+{server.spectators} {m.serverlist_spectators().toLowerCase()}
						{/if}
					</div>
				</div>
			</div>
			{#if !canJoin}
				<div role="alert" class="alert alert-soft alert-warning">
					<TriangleAlert size={18} />
					<span>{m.serverlist_requires_version({ version: server.version })}</span>
				</div>
			{/if}

			<!-- Details -->
			<ul class="list bg-base-200 rounded-box">
				<li class="list-row items-center gap-3 py-2">
					<Lock size={16} class="text-base-content/50 shrink-0" />
					<span class="text-base-content/60">{m.serverlist_password()}</span>
					<span class="text-end font-medium">
						{server.hasPassword ? m.common_yes() : m.common_no()}
					</span>
				</li>
				<li class="list-row items-center gap-3 py-2">
					<Globe size={16} class="text-base-content/50 shrink-0" />
					<span class="text-base-content/60">{m.join_server_host()}</span>
					<span class="text-end font-mono text-sm">{server.ip}:{server.lobbyPort}</span>
				</li>
				{#if showCountry && countryLabel}
					<li class="list-row items-center gap-3 py-2">
						<MapPin size={16} class="text-base-content/50 shrink-0" />
						<span class="text-base-content/60">{m.serverlist_country()}</span>
						<span class="text-end">{countryLabel}</span>
					</li>
				{/if}
			</ul>

			<!-- Tags -->
			{#if server.tags.length > 0}
				<div class="flex flex-wrap items-center gap-1.5">
					<Tag size={14} class="text-base-content/40 shrink-0" />
					{#each server.tags as tag (tag)}
						<span class="badge badge-outline badge-sm">{tag}</span>
					{/each}
				</div>
			{/if}
		</div>

		{#snippet actions()}
			<button class="btn btn-ghost" type="button" onclick={() => (open = false)}>
				{m.common_cancel()}
			</button>
			<button class="btn btn-accent gap-1" type="submit" disabled={!canJoin}>
				<Users size={15} />
				{m.common_join()}
			</button>
		{/snippet}
	</Modal>
{/if}