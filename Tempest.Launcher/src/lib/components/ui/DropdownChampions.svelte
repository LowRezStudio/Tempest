<script lang="ts">
	import { event } from "@tauri-apps/api";
	import { onMount } from "svelte";

	let { title, icon, subtitle, subicon, items, selectedChampion } = $props();

	let showDropdown = $state(false);
	let selectedItem: string = $state("");
	let searchQuery: string = $state("");
	let dropdownButton: HTMLButtonElement;

	onMount(() => {
		selectedItem = items[0]["name"];
	});

	const filteredItems = $derived(
		items.filter((item: any) => 
		item.name.toLowerCase().includes(searchQuery.toLowerCase())
	));

	const dropdown = () => {
		showDropdown = !showDropdown;
		if (!showDropdown) {
			searchQuery = "";
		}
	}

	const selectItem = (item: any) => {
		selectedItem = item["name"];
		selectedChampion(item);
		searchQuery = "";
		showDropdown = false;
	}

	// There's probably a billion better ways to do this it was for testing
	const getChampionClassIcon = (championClass: string) => {
		const classIcons = {
			'Damage': '',
			'Flank': '',
			'Tank': 'https://static.wikia.nocookie.net/paladins_gamepedia/images/4/49/Class_Front_Line_Icon.png',
			'Support': 'https://static.wikia.nocookie.net/paladins_gamepedia/images/d/dc/Class_Support_Icon.png',
		};

		switch (championClass) {
			case 'Damage':
			return classIcons.Damage;
			case 'Flank':
				return classIcons.Flank;
			case 'Tank':
				return classIcons.Tank;
			case 'Support':
				return classIcons.Support;
		}
	}

</script>
<link href="https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined" rel="stylesheet" />

<div class="dropdown-wrapper">
	<button class="dropdown" id="dropdown" bind:this={dropdownButton} onclick={dropdown}>
		<div class="dropdown-infos">
			<img src={icon} alt="subicon" style="">
			<div class="dropdown-title">
				<p>{title}</p>
				<div class="dropdown-subtitle">
					<img src={subicon} alt="subicon" style="width: 20px; height: 20px;">
					{subtitle}
				</div>
			</div>
		</div>
		<span class="material-symbols-outlined">
			keyboard_arrow_down
		</span>
	</button>
	<div class="dropdown-menu {showDropdown ? "show-dropdown" : ""}" style="--anchor-name: --dropdown-anchor;">
		<div class="search-area">
			<input type="text" placeholder="Search champion..." bind:value={searchQuery}>
			<span class="material-symbols-outlined">search</span>
		</div>
		<hr>
		<div class="items">
			{#each filteredItems as item }
				<button class="item {selectedItem === item["name"] ? "selectedItem" : ""}" onclick={() => selectItem(item)}>
					<div class="item-infos">
						<img src={item["icon"]} alt="itemicon">
						<div class="item-title">
							<p>{item["name"]}</p>
							<div class="item-subtitle">
								<img src={getChampionClassIcon(item["class"])} alt="subicon" style="width: 20px; height: 20px;">
								<p>{item["class"]}</p>
							</div>
						</div>
					</div>
					{#if selectedItem === item["name"]}
						<span class="material-symbols-outlined">check</span>
					{/if}
				</button>
			{/each}
		</div>
	</div>
</div>

<style>
	.dropdown {
		display: flex;
		padding: 10px;
		justify-content: space-between;
		align-items: center;
		width: 215px;
		max-height: 55px;
		border-radius: 10px;
		/* background: linear-gradient(270deg, #0f92ce88, #1f72a488, #137da988); */
		background-color: rgba(0, 0, 0, 0.5);
		anchor-name: --dropdown-anchor;
		box-sizing: border-box;
	}
	.dropdown:hover {
		box-shadow: 
			#1cc6fb 0px 0px 16px 0.15rem,
			rgba(0, 0, 0, 0.4) 0px 3px 12px 0px,
			inset rgba(255, 255, 255, 0.3) 0px 1px 0px 0px;
			outline: 2px solid var(--color-primary);
	}
	.dropdown-infos {
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 10px;
	}
	.dropdown-infos > img {
		width: 40px;
		height: 40px;
		border: 2px solid var(--color-primary);
		border-radius: 5px;
	}
	.dropdown-title {
		display: flex;
		flex-direction: column;
	}
	.dropdown-title > p {
		font-size: 14px;
		text-shadow: 1px 1px 2px var(--color-primary), 0 0 25px blue, 0 0 5px darkblue;
		text-transform: uppercase;
        font-weight: 600;
	}
	.dropdown-subtitle {
		font-size: 11px;
		display: flex;
		align-items: center;
		gap: 3px;
		color: rgba(255, 255, 255, 0.7);
	}
	.dropdown-menu {
		position: absolute;
		top: anchor(--dropdown-anchor bottom);
		left: anchor(--dropdown-anchor left);
		width: 215px;
		display: flex;
		flex-direction: column;
		justify-content: center;
		background-color: rgba(0, 0, 0, 0.5);
		padding: 10px;
		border-radius: 10px;
		transform: scaleY(0);
		transform-origin: top;
		transition: transform 0.1s var(--curve);
		z-index: 1000;
		margin-top: 5px;
		box-sizing: border-box;
		backdrop-filter: blur(14px);
	}
	.dropdown-menu.show-dropdown {
		transform: scaleY(1);
	}
	.search-area {
		display: flex;
		justify-content: center;
		align-items: center;
		margin-bottom: 5px;
		gap: 5px;
		font-size: 14px;
	}
	input {
		width: 100%;
		background-color: rgba(255, 255, 255, 0.1);
		outline: 0;
		border: 0;
		border-radius: 7px;
		padding: 5px 10px;
		color: aliceblue;
	}
	input:hover {
		background-color: rgba(255, 255, 255, 0.15);
		outline: 0;
		border: 0;
	}
	input:focus {
		background-color: rgba(255, 255, 255, 0.15);
	}
	.items {
		display: flex;
		flex-direction: column;
	}
	.item {
		display: flex;
		justify-content: space-between;
		align-items: center;
		width: 100%;
		padding-right: 5px;
		border-radius: 10px;
	}
	.item:hover {
		box-shadow: 
			#1cc6fb 0px 0px 16px 0.15rem,
			rgba(0, 0, 0, 0.4) 0px 3px 12px 0px,
			inset rgba(255, 255, 255, 0.3) 0px 1px 0px 0px;
			outline: 2px solid var(--color-primary);
	}
	.item.selectedItem {
		background: linear-gradient(#0f92cecc, #1f72a4cc, #137da9cc);
		box-shadow: #0f92ce 0px 0px 1rem 0.1rem;
		border-color: #0f92ce;
	}
	.item-infos {
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 10px;
		padding: 5px 5px;
	}
	.item-infos > img {
		width: 40px;
		height: 40px;
		border: 2px solid var(--color-primary);
		border-radius: 5px;
	}
	.item-title {
		display: flex;
		flex-direction: column;
	}
	.item-subtitle {
		font-size: 11px;
		display: flex;
		align-items: center;
		gap: 3px;
		color: rgba(255, 255, 255, 0.7);
	}
	hr {
		margin: 5px 0 15px 0;
	}
	button {
		all: unset;
		cursor: pointer;
	}
</style>
