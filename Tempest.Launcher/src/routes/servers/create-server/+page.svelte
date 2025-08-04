<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Input from "$lib/components/ui/Input.svelte";
	import gamemodesData from "$lib/assets/gamemodes/gamemodes.json";

	// Game settings state
	let selectedGamemode = $state(gamemodesData[0]);
	let selectedMap = $state("Splitstone Quarry");
	let serverName = $state("My Custom Game");
	let password = $state("");

	const mapsByGamemode: Record<string, string[]> = {
		"Siege": [
            "Brightmarsh",
			"Fish Market",
            "Frog Isles",
			"Frozen Guard",
            "Ice Mines",
			"Jaguar Falls",
			"Serpent Beach",
			"Splitstone Quarry",
            "Stone Keep",
            "Timber Mill"
		],
		"Onslaught": [
            "Brightmarsh",
			"Jaguar Falls",
			"Primal Court",
			"Snowfall Junction",
		],
		"Payload": [
            "Frostbite Cavern",
            "Greenwood Outpost",
            "Hidden Temple"
        ],
		"Total Mayhem": [
            "Brightmarsh",
			"Fish Market",
            "Frog Isles",
			"Frozen Guard",
            "Ice Mines",
			"Jaguar Falls",
			"Serpent Beach",
			"Splitstone Quarry",
            "Stone Keep",
            "Timber Mill"
        ],
	};

	const availableMaps = $derived(
		mapsByGamemode[selectedGamemode.name] || mapsByGamemode["Siege"],
	);

	$effect(() => {
		if (selectedGamemode && availableMaps.length > 0) {
			selectedMap = availableMaps[0];
		}
	});

	const mapImages: Record<string, string> = {
        "Brightmarsh":
            "https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Atrium.png",
        "Fish Market":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Village.png",
        "Frog Isles":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Isle.png",
        "Frostbite Cavern":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_FrostbiteCaverns.png",
        "Frozen Guard":
            "https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_NRIgloo.png",
        "Greenwood Outpost":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Payload_Forest.png",
        "Hidden Temple":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Payload_Ruins.png",
        "Ice Mines":
            "https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_NRMines.png",
		"Jaguar Falls":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Temple.png",
		"Primal Court":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_TropicalArena.png",
        "Serpent Beach":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Beach.png",
        "Snowfall Junction":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_IceArena.png",
        "Splitstone Quarry":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Quarry.png",
		"Stone Keep":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Castle.png",
        "Timber Mill":
			"https://raw.githubusercontent.com/PaladinsDev/Assets/refs/heads/master/loading-screens/Loading_Spiral.png",
	};

	const gamemodeDescriptions: Record<string, string> = {
		"Siege":
			"Two teams face off against each other. Control the capture point and then push the payload to victory.",
		"Onslaught":
			"Two teams face off against each other. Control the capture point, first team to reach the score limit wins.",
		"Payload":
			"Escort the payload through checkpoints to the enemy base while the defending team tries to stop you.",
		"Total Mayhem":
			"Variant of Siege with reduced cooldowns, increased ultimate charge and increased health.",
	};

	function createServer() {
		const serverConfig = {
			name: serverName,
			gamemode: selectedGamemode.name,
			map: selectedMap,
			password: password || null,
		};

		console.log("Creating server with config:", serverConfig);
	}
</script>

<div class="container">
	<div class="main-content">
		<!-- Left Panel: Map Preview & Gamemode Info -->
        <div class="left-panel">
            <div class="header">
                <Button kind="icon" title="Go back" href="/servers">{"<"}</Button>
                <h1>Create Server</h1>
            </div>
            <div class="preview-panel">
                <div class="map-preview">
                    <img src={mapImages[selectedMap]} alt={selectedMap} class="map-image" />
                    <div class="map-overlay">
                        <h2>{selectedMap}</h2>
                        <div class="gamemode-badge">
                            <img
                                src={selectedGamemode.icon}
                                alt={selectedGamemode.name}
                                class="gamemode-icon"
                            />
                            <span>{selectedGamemode.name}</span>
                        </div>
                    </div>
                </div>
                <div class="gamemode-description">
                    <h3>Game Rules</h3>
                    <p>{gamemodeDescriptions[selectedGamemode.name]}</p>
                </div>
            </div>
            <div class="action-buttons">
				<Button
					onclick={createServer}
					style="width: 100%; font-size: 16px; padding: 12px;"
				>
					Create Server
				</Button>
			</div>
        </div>

		<!-- Right Panel: Game Settings -->
		<div class="settings-panel">
			<div class="settings-section">
				<h3>Server Details</h3>
				<div class="setting-group">
					<label for="server-name">Server Name</label>
					<Input
						id="server-name"
						bind:value={serverName}
						placeholder="Enter server name"
					/>
				</div>
				<div class="setting-group">
					<label for="password">Password (Optional)</label>
					<Input
						id="password"
						type="password"
						bind:value={password}
					/>
				</div>
			</div>

            <div class="settings-section">
				<h3>Game Mode & Map</h3>
				<div class="setting-group">
					<label for="gamemode-option">Select Game Mode</label>
					<div class="gamemode-selector">
						{#each gamemodesData as gamemode}
							<button
								class="gamemode-option {selectedGamemode.name === gamemode.name
									? 'selected'
									: ''}"
								onclick={() => (selectedGamemode = gamemode)}
							>
								<img src={gamemode.icon} alt={gamemode.name} />
								<span>{gamemode.name}</span>
							</button>
						{/each}
					</div>
				</div>
				<div class="setting-group">
					<label for="map-select">Select Map</label>
					<select
						id="map-select"
						bind:value={selectedMap}
						class="custom-select"
					>
						{#each availableMaps as map}
							<option value={map}>{map}</option>
						{/each}
					</select>
				</div>
			</div>
		</div>
	</div>
</div>

<style>
	.header {
		display: flex;
		justify-content: start;
		align-items: center;
		gap: 1rem;
	}

	.header h1 {
		text-shadow:
			1px 1px 2px var(--color-primary),
			0 0 25px darkblue,
			0 0 5px #1cc6fb;
		font-size: 2rem;
	}

	.container {
		width: 100%;
		max-width: 1400px;
		margin: 0 auto;
		padding: 0 1rem;
	}

	.main-content {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 2rem;
		align-items: start;
	}

    .left-panel {
        display: flex;
        flex-direction: column;
        gap: 20px;
    }

	/* Left Panel - Map Preview */
	.preview-panel {
		background-color: var(--bg-surface);
		border-radius: var(--border-radius);
		overflow: hidden;
		border: 2px solid var(--color-primary);
		box-shadow: rgba(0, 0, 0, 0.3) 0px 3px 12px 0px;
	}

	.map-preview {
		position: relative;
		height: 300px;
		overflow: hidden;
	}

	.map-image {
		width: 100%;
		height: 100%;
		object-fit: cover;
		transition: transform 0.3s ease;
	}

	.map-preview:hover .map-image {
		transform: scale(1.05);
	}

	.map-overlay {
		position: absolute;
		bottom: 0;
		left: 0;
		right: 0;
		background: linear-gradient(transparent, rgba(0, 0, 0, 0.8));
		padding: 2rem 1.5rem 1.5rem;
		color: white;
	}

	.map-overlay h2 {
		margin: 0 0 0.5rem 0;
		font-size: 1.5rem;
		text-shadow: 2px 2px 4px rgba(0, 0, 0, 0.8);
	}

	.gamemode-badge {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.gamemode-icon {
		width: 24px;
		height: 24px;
		border-radius: 4px;
	}

	.gamemode-description {
		padding: 1.5rem;
	}

	.gamemode-description h3 {
		margin: 0 0 1rem 0;
		color: var(--color-primary);
		font-size: 1.2rem;
	}

	.gamemode-description p {
		margin: 0;
		line-height: 1.6;
		color: rgba(255, 255, 255, 0.9);
	}

	/* Right Panel - Settings */
	.settings-panel {
		display: flex;
		flex-direction: column;
		gap: 1.5rem;
        padding-top: 1rem;
	}

	.settings-section {
		background-color: var(--bg-surface);
		border-radius: var(--border-radius);
		padding: 1.5rem;
		border: 2px solid var(--color-primary);
		box-shadow: rgba(0, 0, 0, 0.3) 0px 3px 12px 0px;
	}

	.settings-section h3 {
		margin: 0 0 1rem 0;
		color: var(--color-primary);
		font-size: 1.2rem;
		text-shadow: 1px 1px 2px rgba(0, 0, 0, 0.5);
	}

	.setting-group {
		margin-bottom: 1rem;
	}

	.setting-group:last-child {
		margin-bottom: 0;
	}

	.setting-group label {
		display: block;
		margin-bottom: 0.5rem;
		font-weight: 600;
		color: rgba(255, 255, 255, 0.9);
	}

	.gamemode-selector {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: 0.5rem;
	}

	.gamemode-option {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.75rem;
		background: linear-gradient(#0f92cecc, #1f72a4cc, #137da9cc);
		border: 2px solid var(--color-primary);
		border-radius: var(--border-radius);
		cursor: pointer;
		transition: all 0.3s ease;
		font-size: 0.9rem;
		font-weight: 600;
	}

	.gamemode-option img {
		width: 24px;
		height: 24px;
		border-radius: 4px;
	}

	.gamemode-option:hover {
		background: linear-gradient(#1cc6fbcc, #0194d4cc, #00a2dacc);
		box-shadow: #0f92ce 0px 0px 1rem 0.1rem;
		border-color: #1cc6fb;
		transform: translateY(-1px);
	}

	.gamemode-option.selected {
		background: linear-gradient(#1cc6fbcc, #0194d4cc, #00a2dacc);
		box-shadow: #1cc6fb 0px 0px 1rem 0.2rem;
		border-color: #1cc6fb;
	}

	.custom-select {
		width: 100%;
		background: linear-gradient(#0f92cecc, #1f72a4cc, #137da9cc);
		border: 2px solid var(--color-primary);
		border-radius: var(--border-radius);
		padding: 0.75rem 1rem;
		color: inherit;
		font-size: 15px;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.3s ease;
	}

	.custom-select:hover {
		background: linear-gradient(#1cc6fbcc, #0194d4cc, #00a2dacc);
		box-shadow: #0f92ce 0px 0px 1rem 0.1rem;
		border-color: #1cc6fb;
	}

	.custom-select:focus {
		outline: 2px solid #1cc6fb;
		outline-offset: -2px;
	}

	.action-buttons {
		margin-top: 1rem;
	}

	button {
		all: unset;
		cursor: pointer;
	}
</style>
