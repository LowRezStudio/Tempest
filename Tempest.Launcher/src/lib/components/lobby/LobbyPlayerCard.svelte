<script lang="ts">
	import { ChevronLeft, ChevronRight } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	// ponytail: Slanted border drawn as SVG polygons with perfectly horizontal Y-axis caps to match the background slants
	interface Props {
		displayName: string;
		championIconFolderName: string;
		status: string;
		team?: "left" | "right";
		compact?: boolean;
		canSwitchTeam?: boolean;
		onSwitchTeam?: () => void;
	}

	let {
		displayName,
		championIconFolderName: champion,
		status,
		team = "left",
		compact = false,
		canSwitchTeam = false,
		onSwitchTeam = () => {},
	}: Props = $props();

	let hasChampion = $derived(!!champion);

	function handleKeydown(e: KeyboardEvent) {
		if (!canSwitchTeam) return;
		if (e.key === "Enter" || e.key === " ") {
			e.preventDefault();
			onSwitchTeam();
		}
	}
</script>

{#if team === "left"}
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		class="flex items-center gap-3 relative transition-all duration-200
			{compact ? 'p-1.5' : 'p-2'}
			{canSwitchTeam ? 'cursor-pointer group' : ''}"
		role={canSwitchTeam ? "button" : undefined}
		tabindex={canSwitchTeam ? 0 : undefined}
		title={canSwitchTeam ? m.lobby_switch_team() : undefined}
		onclick={canSwitchTeam ? onSwitchTeam : undefined}
		onkeydown={handleKeydown}
	>
		<!-- Clipped Background -->
		<div
			class="absolute inset-0 bg-gradient-to-r from-blue-950/40 via-base-200/90 to-base-200/90 border transition-colors duration-200 {canSwitchTeam ? 'border-blue-400/40 bg-blue-500/5' : 'border-blue-500/20'}"
			style="clip-path: polygon(0% 0%, calc(100% - 80px) 0%, 100% 100%, 0% 100%);"
		></div>

		<div class="relative flex-shrink-0 z-10">
			<img
				src={`/champions/${champion || "Generic"}/icon.webp`}
				alt={champion || "No Champion"}
				class="rounded-none ring-2 ring-blue-500/30 object-cover"
				class:w-10={compact}
				class:h-10={compact}
				class:w-12={!compact}
				class:h-12={!compact}
				loading="lazy"
				onerror={(e) => {
					(e.currentTarget as HTMLImageElement).src = "/champions/Generic/icon.webp";
				}}
			/>
		</div>
		<div class="min-w-0 flex-1 z-10 pr-[80px]">
			<p class="font-semibold truncate text-white leading-tight {compact ? 'text-sm' : 'text-base'}">
				{displayName}
			</p>
			<p class="text-xs opacity-75 truncate leading-none mt-1 {hasChampion ? 'text-blue-400' : 'text-white/50'}">
				{status}
			</p>
		</div>
		<!-- Slanted Edge Border (unclipped z-20 overlay with horizontal Y-axis caps) -->
		<svg class="absolute right-0 top-0 bottom-0 h-full w-[80px] text-blue-500 z-20" viewBox="0 0 80 100" preserveAspectRatio="none">
			<polygon points="0,0 26,0 80,100 54,100" fill="currentColor" />
		</svg>
		{#if canSwitchTeam}
			<ChevronRight
				size={16}
				class="absolute right-[32px] top-1/2 -translate-y-1/2 z-30 text-blue-300/50 group-hover:text-blue-100 group-hover:opacity-100 transition-colors"
			/>
		{/if}
	</div>
{:else}
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		class="flex flex-row-reverse items-center gap-3 relative transition-all duration-200 text-right
			{compact ? 'p-1.5' : 'p-2'}
			{canSwitchTeam ? 'cursor-pointer group' : ''}"
		role={canSwitchTeam ? "button" : undefined}
		tabindex={canSwitchTeam ? 0 : undefined}
		title={canSwitchTeam ? m.lobby_switch_team() : undefined}
		onclick={canSwitchTeam ? onSwitchTeam : undefined}
		onkeydown={handleKeydown}
	>
		<!-- Clipped Background -->
		<div
			class="absolute inset-0 bg-gradient-to-l from-red-950/40 via-base-200/90 to-base-200/90 border transition-colors duration-200 {canSwitchTeam ? 'border-red-400/40 bg-red-500/5' : 'border-red-500/20'}"
			style="clip-path: polygon(80px 0%, 100% 0%, 100% 100%, 0% 100%);"
		></div>

		<div class="relative flex-shrink-0 z-10">
			<img
				src={`/champions/${champion || "Generic"}/icon.webp`}
				alt={champion || "No Champion"}
				class="rounded-none ring-2 ring-red-500/30 object-cover"
				class:w-10={compact}
				class:h-10={compact}
				class:w-12={!compact}
				class:h-12={!compact}
				loading="lazy"
				onerror={(e) => {
					(e.currentTarget as HTMLImageElement).src = "/champions/Generic/icon.webp";
				}}
			/>
		</div>
		<div class="min-w-0 flex-1 z-10 pl-[80px]">
			<p class="font-semibold truncate text-white leading-tight {compact ? 'text-sm' : 'text-base'}">
				{displayName}
			</p>
			<p class="text-xs opacity-75 truncate leading-none mt-1 {hasChampion ? 'text-red-400' : 'text-white/50'}">
				{status}
			</p>
		</div>
		<!-- Slanted Edge Border (unclipped z-20 overlay with horizontal Y-axis caps) -->
		<svg class="absolute top-0 left-0 bottom-0 h-full w-[80px] text-red-500 z-20" viewBox="0 0 80 100" preserveAspectRatio="none">
			<polygon points="54,0 80,0 26,100 0,100" fill="currentColor" />
		</svg>
		{#if canSwitchTeam}
			<ChevronLeft
				size={16}
				class="absolute left-[32px] top-1/2 -translate-y-1/2 z-30 text-red-300/50 group-hover:text-red-100 group-hover:opacity-100 transition-colors"
			/>
		{/if}
	</div>
{/if}
