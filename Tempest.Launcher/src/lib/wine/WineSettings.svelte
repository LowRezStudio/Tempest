<script lang="ts">
	import { Info } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import { platform } from "@tauri-apps/plugin-os";
	import { createCommand } from "$lib/core/command";
	import { m } from "$lib/paraglide/messages";
	import {
		gamescopeArgs,
		protonPath,
		useGamescope,
		useSteamRuntime,
		winePath,
		wineRuntime,
		type WineRuntimeSetting,
	} from "$lib/stores/settings.svelte";
	import { which } from "$lib/tauri/which";

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

<div class="flex flex-col gap-4" class:hidden={isWindows} aria-hidden={isWindows}>
	<div class="form-control">
		<label for="wine-runtime-select" class="label py-0.5">
			<span class="label-text text-sm">{m.settings_wine_runtime()}</span>
		</label>
		<select
			id="wine-runtime-select"
			class="select select-bordered w-full"
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
	</div>

	<div class="form-control">
		<label for="wine-path-input" class="label py-0.5">
			<span class="label-text text-sm">{m.settings_wine_executable()}</span>
		</label>
		<div class="join w-full">
			<input
				id="wine-path-input"
				type="text"
				class="input input-bordered join-item flex-1 font-mono"
				bind:value={winePath.value}
				placeholder={m.settings_wine_executable_placeholder()}
			/>
			<button type="button" class="btn btn-accent join-item" onclick={browseWinePath}>
				{m.common_browse()}
			</button>
			<button type="button" class="btn btn-secondary join-item" onclick={detectWine}>
				{m.settings_wine_autodetect()}
			</button>
		</div>
	</div>

	{#if showProtonPath}
		<div class="form-control">
			<label for="proton-path-input" class="label py-0.5">
				<span class="label-text text-sm">{m.settings_wine_proton_directory()}</span>
			</label>
			<div class="join w-full">
				<input
					id="proton-path-input"
					type="text"
					class="input input-bordered join-item flex-1 font-mono"
					bind:value={protonPath.value}
					placeholder={m.settings_wine_proton_placeholder()}
				/>
				<button type="button" class="btn btn-accent join-item" onclick={browseProtonPath}>
					{m.common_browse()}
				</button>
			</div>
		</div>
	{/if}

	{#if showSteamRuntime}
		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input
					type="checkbox"
					class="toggle toggle-accent"
					bind:checked={useSteamRuntime.value}
				/>
				<span class="label-text text-sm">{m.settings_wine_steam_runtime()}</span>
			</label>
			<p class="mt-1 text-xs opacity-60">
				{m.settings_wine_steam_runtime_description()}
			</p>
		</div>
	{/if}

	{#if isLinux}
		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input
					type="checkbox"
					class="toggle toggle-accent"
					bind:checked={useGamescope.value}
					disabled={!isGamescopeInstalled}
				/>
				<span class="label-text text-sm">{m.settings_wine_use_gamescope()}</span>
			</label>
			{#if !isGamescopeInstalled}
				<p class="mt-1 text-xs opacity-60">{m.settings_wine_gamescope_not_installed()}</p>
			{/if}
		</div>

		<div class="form-control">
			<div class="flex items-center gap-1">
				<label for="gamescope-args-input" class="label py-0.5">
					<span class="label-text text-sm">{m.settings_wine_gamescope_args()}</span>
				</label>
				<button
					type="button"
					class="btn btn-ghost btn-xs btn-circle opacity-60 hover:opacity-100"
					onclick={() =>
						openUrl("https://github.com/ValveSoftware/gamescope/blob/master/README.md")}
					title="Gamescope README"
				>
					<Info size={14} />
				</button>
			</div>
			<input
				id="gamescope-args-input"
				type="text"
				class="input input-bordered w-full font-mono"
				bind:value={gamescopeArgs.value}
				placeholder={m.settings_wine_gamescope_args_placeholder()}
				disabled={!isGamescopeInstalled}
			/>
		</div>
	{/if}
</div>
