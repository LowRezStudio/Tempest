<script lang="ts">
	import { Gamepad2, Lock, MapPin, Swords, TriangleAlert, Users } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { CountryCode, type ServerListing } from "$lib/rpc";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { getMapsForVersion } from "$lib/utils/versions";

	interface Props {
		server: ServerListing;
		showCountry?: boolean;
		onclick?: (server: ServerListing) => void;
	}

	let { server, showCountry = false, onclick }: Props = $props();

	const canJoin = $derived(
		Object.values(instanceMap.value).some((i) => i && i.version === server.version),
	);

	function findMapName(): string {
		if (!server.map && !server.mapId) return m.common_na();
		if (server.map) return server.map;
		const map = getMapsForVersion(server.version).find((m) => m.id === server.mapId);
		if (!map) return server.mapId ?? m.common_na();
		return map.displayName;
	}

	function findGamemodeName(): string {
		if (server.gamemode.startsWith("TempestMp.")) {
			return server.gamemode.slice(server.gamemode.indexOf(".") + 1);
		}
		return server.gamemode;
	}

	function toFlagEmoji(code: string): string {
		if (!code || code.length !== 2) return "";
		return [...code.toUpperCase()]
			.map((char) => String.fromCodePoint(127397 + char.charCodeAt(0)))
			.join("");
	}

	function formatCountryLabel(code: string): string {
		if (code === "COUNTRY_CODE_UNSPECIFIED") return "";
		const flag = toFlagEmoji(code);
		return flag ? `${flag} ${code}` : code;
	}

	const showFooter = $derived(
		server.tags.length > 0 || server.spectators > 0 || (showCountry && server.country !== 0),
	);

	function handleClick() {
		onclick?.(server);
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<button
	type="button"
	class="bg-base-200 hover:bg-base-300 w-full cursor-pointer rounded-lg p-4 text-left transition-colors"
	class:opacity-60={!canJoin}
	onclick={handleClick}
>
	<div class="flex flex-col gap-2.5">
		<!-- Title + player capacity -->
		<div class="flex items-start justify-between gap-2">
			<h3 class="flex min-w-0 items-center gap-1.5 text-base font-bold">
				{#if server.hasPassword}
					<Lock size={14} class="text-warning shrink-0" />
				{/if}
				<span class="truncate">{server.name}</span>
			</h3>
			<div class="flex shrink-0 items-center gap-1.5">
				{#if !canJoin}
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<span
						class="tooltip tooltip-left"
						data-tip={m.serverlist_requires_version({ version: server.version })}
						onclick={(e) => e.stopPropagation()}
						onkeydown={(e) => e.stopPropagation()}
					>
						<TriangleAlert size={16} class="text-warning" />
					</span>
				{/if}
				<span class="badge badge-soft badge-sm gap-1">
					<Users size={12} />
					{server.players}/{server.maxPlayers}
				</span>
			</div>
		</div>

		<!-- Gamemode · Map · Version -->
		<div class="text-base-content/60 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm">
			<span class="inline-flex items-center gap-1">
				<Swords size={13} />
				{findGamemodeName()}
			</span>
			<span>·</span>
			<span class="inline-flex items-center gap-1">
				<MapPin size={13} />
				{findMapName()}
			</span>
			<span>·</span>
			<span class="inline-flex items-center gap-1">
				<Gamepad2 size={13} />
				{server.version}
			</span>
		</div>

		<!-- Tags + meta -->
		{#if showFooter}
			<div class="flex flex-wrap items-center gap-1.5">
				{#if server.tags.length > 0}
					{#each server.tags as tag (tag)}
						<span class="badge badge-outline badge-xs">{tag}</span>
					{/each}
				{/if}
				<div class="ml-auto flex items-center gap-2">
					{#if server.spectators > 0}
						<span class="text-base-content/50 text-xs">
							+{server.spectators}
							{m.serverlist_spectators().toLowerCase()}
						</span>
					{/if}
					{#if showCountry && server.country !== 0}
						<span class="text-base-content/60 text-xs">
							{formatCountryLabel(CountryCode[server.country])}
						</span>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</button>
