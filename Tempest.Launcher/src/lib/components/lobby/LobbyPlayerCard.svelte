<script lang="ts">
	// ponytail: Slanted border drawn as SVG polygons with perfectly horizontal Y-axis caps to match the background slants
	interface Props {
		displayName: string;
		championIconFolderName: string;
		status: string;
		team?: "left" | "right";
		compact?: boolean;
	}

	let {
		displayName,
		championIconFolderName: champion,
		status,
		team = "left",
		compact = false,
	}: Props = $props();

	let hasChampion = $derived(!!champion);
</script>

{#if team === "left"}
	<div
		class="flex items-center gap-3 relative transition-all duration-200
			{compact ? 'p-1.5' : 'p-2'}"
	>
		<!-- Clipped Background -->
		<div
			class="absolute inset-0 bg-gradient-to-r from-blue-950/40 via-base-200/90 to-base-200/90 border border-blue-500/20"
			style="clip-path: polygon(0% 0%, calc(100% - 24px) 0%, 100% 100%, 0% 100%);"
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
		<div class="min-w-0 flex-1 z-10 pr-8">
			<p class="font-semibold truncate text-white leading-tight {compact ? 'text-sm' : 'text-base'}">
				{displayName}
			</p>
			<p class="text-xs opacity-75 truncate leading-none mt-1 {hasChampion ? 'text-blue-400' : 'text-white/50'}">
				{status}
			</p>
		</div>
		<!-- Slanted Edge Border (unclipped z-20 overlay with horizontal Y-axis caps) -->
		<svg class="absolute right-0 top-0 bottom-0 h-full w-6 text-blue-500 z-20" viewBox="0 0 24 100" preserveAspectRatio="none">
			<polygon points="0,0 8,0 24,100 16,100" fill="currentColor" />
		</svg>
	</div>
{:else}
	<div
		class="flex flex-row-reverse items-center gap-3 relative transition-all duration-200 text-right
			{compact ? 'p-1.5' : 'p-2'}"
	>
		<!-- Clipped Background -->
		<div
			class="absolute inset-0 bg-gradient-to-l from-red-950/40 via-base-200/90 to-base-200/90 border border-red-500/20"
			style="clip-path: polygon(24px 0%, 100% 0%, 100% 100%, 0% 100%);"
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
		<div class="min-w-0 flex-1 z-10 pl-8">
			<p class="font-semibold truncate text-white leading-tight {compact ? 'text-sm' : 'text-base'}">
				{displayName}
			</p>
			<p class="text-xs opacity-75 truncate leading-none mt-1 {hasChampion ? 'text-red-400' : 'text-white/50'}">
				{status}
			</p>
		</div>
		<!-- Slanted Edge Border (unclipped z-20 overlay with horizontal Y-axis caps) -->
		<svg class="absolute top-0 left-0 bottom-0 h-full w-6 text-red-500 z-20" viewBox="0 0 24 100" preserveAspectRatio="none">
			<polygon points="16,0 24,0 8,100 0,100" fill="currentColor" />
		</svg>
	</div>
{/if}
