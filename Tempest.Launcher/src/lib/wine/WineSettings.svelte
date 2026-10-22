<script lang="ts">
	import { Info, Maximize, Package } from "@lucide/svelte";
	import SwitchRow from "$lib/components/ui/SwitchRow.svelte";
	import { createCommand } from "$lib/core/command";
	import { m } from "$lib/paraglide/messages";
	import { open, openUrl, platform } from "$lib/platform/api";
	import { which } from "$lib/platform/which";
	import {
		gamescopeArgs,
		protonPath,
		useGamescope,
		useSteamRuntime,
		winePath,
		wineRuntime,
		type WineRuntimeSetting,
	} from "$lib/stores/settings.svelte";

	type DetectedProton = { path: string; name: string };

	let gamescopePath = $state<string | null>(null);
	let isGamescopeInstalled = $derived(gamescopePath !== null);
	let detectedProtons = $state<DetectedProton[]>([]);
	let protonListLoaded = $state(false);
	let selectedRuntime = $state("auto");

	$effect(() => {
		which("gamescope").then((path) => {
			gamescopePath = path;
		});
	});

	$effect(() => {
		if (isWindows || protonListLoaded) return;
		protonListLoaded = true;
		void (async () => {
			try {
				const result = await createCommand(["wine", "list-proton"]).execute();
				detectedProtons = result.stdout
					.split("\n")
					.map((l) => l.trim())
					.filter((l) => l.length > 0)
					.map((p) => ({ path: p, name: p.split("/").filter(Boolean).pop() ?? p }));
			} catch {
				// detection is best-effort; the custom path input still works
			}
		})();
	});

	// Keep the select in sync with the persisted stores.
	$effect(() => {
		const runtime = wineRuntime.value;
		if (runtime === "auto") selectedRuntime = "auto";
		else if (runtime === "wine") selectedRuntime = "wine";
		else {
			const path = protonPath.value ?? "";
			selectedRuntime =
				path && detectedProtons.some((d) => d.path === path) ? `proton:${path}` : "custom";
		}
	});

	function onRuntimeChange(value: string) {
		if (value === "auto") wineRuntime.value = "auto";
		else if (value === "wine") wineRuntime.value = "wine";
		else if (value.startsWith("proton:")) {
			protonPath.value = value.slice("proton:".length);
			wineRuntime.value = "proton";
		} else {
			wineRuntime.value = "proton"; // custom path, keep the current one
		}
	}

	async function browseWinePath() {
		const selected = await open({
			directory: false,
			multiple: false,
			title: m.settings_wine_select_executable(),
		});

		if (selected) {
			winePath.value = selected;
		}
	}

	async function detectWine() {
		const detected = await which("wine");
		if (detected) {
			winePath.value = detected;
		}
	}

	async function browseProtonPath() {
		const selected = await open({
			directory: true,
			multiple: false,
			title: m.settings_wine_proton_select_directory(),
		});

		if (selected) {
			protonPath.value = selected;
			wineRuntime.value = "proton";
		}
	}

	const isWindows = platform() === "windows";
	const isLinux = platform() === "linux";
	const showProtonPath = $derived(wineRuntime.value === "proton");
	const showSteamRuntime = $derived(isLinux && wineRuntime.value !== "wine");
	const runtimeOptions: { value: WineRuntimeSetting; label: string }[] = [
		{ value: "auto", label: m.settings_wine_runtime_auto() },
		{ value: "wine", label: m.settings_wine_runtime_system() },
	];
</script>

<div class="flex flex-col gap-3" class:hidden={isWindows} aria-hidden={isWindows}>
	<label class="flex flex-col gap-1">
		<span class="text-base-content/60 text-xs">{m.settings_wine_runtime()}</span>
		<select
			class="select select-sm w-full"
			value={selectedRuntime}
			onchange={(e) => onRuntimeChange(e.currentTarget.value)}
		>
			{#each runtimeOptions as option}
				<option value={option.value}>{option.label}</option>
			{/each}
			{#each detectedProtons as proton}
				<option value={`proton:${proton.path}`}>
					{m.settings_wine_proton_option({ name: proton.name })}
				</option>
			{/each}
			<option value="custom">{m.settings_wine_runtime_custom()}</option>
		</select>
	</label>

	<div class="flex flex-col gap-1">
		<label for="wine-path-input" class="text-base-content/60 text-xs"
			>{m.settings_wine_executable()}</label
		>
		<div class="join w-full">
			<input
				id="wine-path-input"
				type="text"
				class="input input-sm join-item min-w-0 flex-1 font-mono"
				spellcheck="false"
				bind:value={winePath.value}
				placeholder={m.settings_wine_executable_placeholder()}
			/>
			<button type="button" class="btn btn-sm join-item" onclick={browseWinePath}>
				{m.common_browse()}
			</button>
			<button type="button" class="btn btn-sm join-item" onclick={detectWine}>
				{m.settings_wine_autodetect()}
			</button>
		</div>
	</div>

	{#if showProtonPath}
		<div class="flex flex-col gap-1">
			<label for="proton-path-input" class="text-base-content/60 text-xs"
				>{m.settings_wine_proton_directory()}</label
			>
			<div class="join w-full">
				<input
					id="proton-path-input"
					type="text"
					class="input input-sm join-item min-w-0 flex-1 font-mono"
					spellcheck="false"
					bind:value={protonPath.value}
					placeholder={m.settings_wine_proton_placeholder()}
				/>
				<button type="button" class="btn btn-sm join-item" onclick={browseProtonPath}>
					{m.common_browse()}
				</button>
			</div>
		</div>
	{/if}

	{#if showSteamRuntime}
		<SwitchRow
			bind:checked={useSteamRuntime.value}
			icon={Package}
			title={m.settings_wine_steam_runtime()}
			hint={m.settings_wine_steam_runtime_description()}
		/>
	{/if}

	{#if isLinux}
		<SwitchRow
			bind:checked={useGamescope.value}
			icon={Maximize}
			title={m.settings_wine_use_gamescope()}
			hint={isGamescopeInstalled ? undefined : m.settings_wine_gamescope_not_installed()}
			disabled={!isGamescopeInstalled}
		/>

		<div class="flex flex-col gap-1">
			<span class="text-base-content/60 flex items-center gap-1 text-xs">
				<label for="gamescope-args-input">{m.settings_wine_gamescope_args()}</label>
				<button
					type="button"
					class="btn btn-ghost btn-xs btn-circle size-5 min-h-0 opacity-60 hover:opacity-100"
					onclick={() =>
						openUrl("https://github.com/ValveSoftware/gamescope/blob/master/README.md")}
					title="Gamescope README"
				>
					<Info size={12} />
				</button>
			</span>
			<input
				id="gamescope-args-input"
				type="text"
				class="input input-sm w-full font-mono"
				spellcheck="false"
				bind:value={gamescopeArgs.value}
				placeholder={m.settings_wine_gamescope_args_placeholder()}
				disabled={!isGamescopeInstalled}
			/>
		</div>
	{/if}
</div>
