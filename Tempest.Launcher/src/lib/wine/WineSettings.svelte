<script lang="ts">
	import { Info } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import { platform } from "@tauri-apps/plugin-os";
	import { m } from "$lib/paraglide/messages";
	import { gamescopeArgs, useGamescope, winePath } from "$lib/stores/settings";
	import { which } from "$lib/tauri/which";

	let gamescopePath = $state<string | null>(null);
	let isGamescopeInstalled = $derived(gamescopePath !== null);

	$effect(() => {
		which("gamescope").then((path) => {
			gamescopePath = path;
		});
	});

	async function browseWinePath() {
		const selected = await open({
			directory: false,
			multiple: false,
			title: m.settings_wine_select_executable(),
		});

		if (selected) {
			$winePath = selected;
		}
	}

	async function detectWine() {
		const detected = await which("wine");
		if (detected) {
			$winePath = detected;
		}
	}

	let localUseGamescope = $state($useGamescope === "true");

	$effect(() => {
		useGamescope.set(localUseGamescope ? "true" : "false");
	});

	const isWindows = platform() === "windows";
	const isLinux = platform() === "linux";
</script>

<div class="flex flex-col gap-4" class:hidden={isWindows} aria-hidden={isWindows}>
	<div class="form-control">
		<label for="wine-path-input" class="label py-0.5">
			<span class="label-text text-sm">{m.settings_wine_executable()}</span>
		</label>
		<div class="join w-full">
			<input
				id="wine-path-input"
				type="text"
				class="input input-bordered join-item flex-1 font-mono"
				bind:value={$winePath}
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

	{#if isLinux}
		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input
					type="checkbox"
					class="toggle toggle-accent"
					bind:checked={localUseGamescope}
					disabled={!isGamescopeInstalled}
				/>
				<span class="label-text text-sm">{m.settings_wine_use_gamescope()}</span>
			</label>
			{#if !isGamescopeInstalled}
				<p class="text-xs opacity-60 mt-1">{m.settings_wine_gamescope_not_installed()}</p>
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
				bind:value={$gamescopeArgs}
				placeholder={m.settings_wine_gamescope_args_placeholder()}
				disabled={!isGamescopeInstalled}
			/>
		</div>
	{/if}
</div>
