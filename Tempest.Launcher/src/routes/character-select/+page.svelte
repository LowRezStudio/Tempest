<script lang="ts">
	import ChampionSelect from "$lib/components/champions/ChampionSelect.svelte";

	interface Champion {
		name: string;
		iconPath: string;
		fallbackPath: string;
		videoPath: string;
	}

	let selectedChampion = $state<Champion | null>(null);

	function handleChampionSelect(champion: Champion) {
		selectedChampion = champion;
		console.log("Champion selected:", champion);
	}
</script>

<svelte:head>
	<title>Character Selection</title>
</svelte:head>

<div class="h-screen">
	<ChampionSelect onselect={handleChampionSelect} />
</div>
