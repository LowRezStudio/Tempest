<script lang="ts">
	import { fade } from "svelte/transition";
	import { currentWeather } from "$lib/theme/weather.svelte";
	import ChristmasDecorations from "./ChristmasDecorations.svelte";
	import CursorFollower from "./CursorFollower.svelte";
	import HalloweenDecorations from "./HalloweenDecorations.svelte";
	import Snowman from "./Snowman.svelte";
</script>

<!-- In front of the pages (the weather falls behind them) but under dialogs and toasts; clicks go
     through -->
<div class="pointer-events-none absolute inset-0 z-10" aria-hidden="true">
	{#if currentWeather.value?.spooky}
		<div class="absolute inset-0" transition:fade={{ duration: 600 }}>
			<HalloweenDecorations />
		</div>
	{/if}
	{#if currentWeather.value?.festive}
		<div class="absolute inset-0" transition:fade={{ duration: 600 }}>
			<ChristmasDecorations />
		</div>
	{/if}
	{#if currentWeather.value?.kind === "snow"}
		<div class="absolute inset-0" transition:fade={{ duration: 600 }}>
			<CursorFollower gait="waddle" width={88}>
				{#snippet children(look)}
					<Snowman {look} />
				{/snippet}
			</CursorFollower>
		</div>
	{/if}
</div>
