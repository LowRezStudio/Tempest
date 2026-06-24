<script lang="ts">
	import { Info, Settings } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import { platform } from "@tauri-apps/plugin-os";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createAboutInfoQuery } from "$lib/queries/about";
	import {
		defaultInstancePath,
		gamescopeArgs,
		theme,
		useGamescope,
		username,
		winePath,
	} from "$lib/stores/settings";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import { which } from "$lib/tauri/which";
	import type { Theme } from "$lib/stores/settings";

	let activeTab = $state<"general" | "wine" | "advanced">("general");

	let localUsername = $state($username);
	let localPath = $state($defaultInstancePath || "");
	let localTheme = $state<Theme>($theme);
	let localWinePath = $state($winePath || "");
	let localUseGamescope = $state($useGamescope === "true");
	let localGamescopeArgs = $state($gamescopeArgs);
	let gamescopePath = $state<string | null>(null);
	let isGamescopeInstalled = $derived(gamescopePath !== null);

	let buildType = $state<string>(import.meta.env.DEV ? "Development" : "Production");

	$effect(() => {
		username.set(localUsername);
	});
	$effect(() => {
		defaultInstancePath.set(localPath || undefined);
	});
	$effect(() => {
		theme.set(localTheme);
	});
	$effect(() => {
		winePath.set(localWinePath || undefined);
	});
	$effect(() => {
		useGamescope.set(localUseGamescope ? "true" : "false");
	});
	$effect(() => {
		gamescopeArgs.set(localGamescopeArgs);
	});

	$effect(() => {
		which("gamescope").then((path) => {
			gamescopePath = path;
		});
	});

	async function browsePath() {
		const selected = await open({
			directory: true,
			multiple: false,
			title: m.settings_select_instance_folder(),
		});

		if (selected) {
			localPath = selected;
		}
	}

	async function browseWinePath() {
		const selected = await open({
			directory: false,
			multiple: false,
			title: m.settings_wine_select_executable(),
		});

		if (selected) {
			localWinePath = selected;
		}
	}

	async function detectWine() {
		const detected = await which("wine");
		if (detected) {
			localWinePath = detected;
		}
	}

	function resetAll() {
		localStorage.clear();
		location.href = "/";
	}

	const aboutQuery = createAboutInfoQuery();

	let appVersion = $derived(aboutQuery.data?.appVersion ?? "...");
	let osName = $derived(aboutQuery.data?.osName ?? "...");
	let architecture = $derived(aboutQuery.data?.architecture ?? "...");
	let buildDate = $derived(aboutQuery.data?.buildDate ?? "...");

	const isWindows = platform() === "windows";
	const isLinux = platform() === "linux";
	const tabs = $derived([
		{ name: m.settings_general(), value: "general" as const },
		...(isWindows ? [] : [{ name: m.settings_wine(), value: "wine" as const }]),
		{ name: m.settings_advanced(), value: "advanced" as const },
	]);
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header title={m.settings_title()} {tabs} {activeTab} onSelectTab={(tab) => (activeTab = tab)}>
		{#snippet icon()}
			<Settings size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>{m.settings_configure_preferences()}</span>
		{/snippet}
	</Header>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="p-6">
				{#if activeTab === "general"}
					<div class="flex flex-col gap-4">
						<div class="form-control">
							<label for="username-input" class="label py-0.5">
								<span class="label-text text-sm">{m.settings_username()}</span>
							</label>
							<input
								id="username-input"
								type="text"
								class="input input-bordered w-full"
								bind:value={localUsername}
								placeholder={m.settings_username_placeholder()}
							/>
						</div>

						<div class="form-control">
							<label for="theme-input" class="label py-0.5">
								<span class="label-text text-sm">{m.settings_theme()}</span>
							</label>
							<select
								id="theme-input"
								class="select select-bordered w-full"
								bind:value={localTheme}
							>
								<option value="system">{m.settings_theme_system()}</option>
								<option value="mocha">{m.settings_theme_mocha()}</option>
								<option value="latte">{m.settings_theme_latte()}</option>
								{#if localUsername === "Grohk" || localTheme === "legacy"}
									<option value="legacy">{m.settings_theme_legacy()}</option>
								{/if}
							</select>
						</div>

						<div class="form-control">
							<label for="path-input" class="label py-0.5">
								<span class="label-text text-sm"
									>{m.settings_default_instance_path()}</span
								>
							</label>
							<div class="join w-full">
								<input
									id="path-input"
									type="text"
									class="input input-bordered join-item flex-1 font-mono"
									bind:value={localPath}
									placeholder={m.settings_select_directory()}
								/>
								<button
									type="button"
									class="btn btn-accent join-item"
									onclick={browsePath}>{m.common_browse()}</button
								>
							</div>
						</div>
					</div>
				{:else if activeTab === "wine"}
					<div
						class="flex flex-col gap-4"
						class:hidden={isWindows}
						aria-hidden={isWindows}
					>
						<div class="form-control">
							<label for="wine-path-input" class="label py-0.5">
								<span class="label-text text-sm"
									>{m.settings_wine_executable()}</span
								>
							</label>
							<div class="join w-full">
								<input
									id="wine-path-input"
									type="text"
									class="input input-bordered join-item flex-1 font-mono"
									bind:value={localWinePath}
									placeholder={m.settings_wine_executable_placeholder()}
								/>
								<button
									type="button"
									class="btn btn-accent join-item"
									onclick={browseWinePath}
								>
									{m.common_browse()}
								</button>
								<button
									type="button"
									class="btn btn-secondary join-item"
									onclick={detectWine}
								>
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
									<span class="label-text text-sm"
										>{m.settings_wine_use_gamescope()}</span
									>
								</label>
								{#if !isGamescopeInstalled}
									<p class="text-xs opacity-60 mt-1">
										{m.settings_wine_gamescope_not_installed()}
									</p>
								{/if}
							</div>

							<div class="form-control">
								<div class="flex items-center gap-1">
									<label for="gamescope-args-input" class="label py-0.5">
										<span class="label-text text-sm"
											>{m.settings_wine_gamescope_args()}</span
										>
									</label>
									<button
										type="button"
										class="btn btn-ghost btn-xs btn-circle opacity-60 hover:opacity-100"
										onclick={() =>
											openUrl(
												"https://github.com/ValveSoftware/gamescope/blob/master/README.md",
											)}
										title="Gamescope README"
									>
										<Info size={14} />
									</button>
								</div>
								<input
									id="gamescope-args-input"
									type="text"
									class="input input-bordered w-full font-mono"
									bind:value={localGamescopeArgs}
									placeholder={m.settings_wine_gamescope_args_placeholder()}
									disabled={!isGamescopeInstalled}
								/>
							</div>
						{/if}
					</div>
				{:else if activeTab === "advanced"}
					<div class="flex flex-col">
						<div class="flex flex-col gap-4">
							<h2 class="text-xl font-semibold text-error">
								{m.settings_danger_zone()}
							</h2>
							<p class="text-sm">
								{m.settings_reset_warning()}
							</p>
							<div>
								<button class="btn btn-error" onclick={resetAll}
									>{m.settings_clear_all()}</button
								>
							</div>
						</div>

						<div class="divider"></div>

						<div class="flex flex-col gap-4">
							<h2 class="text-xl font-semibold">{m.settings_about()}</h2>
							<div class="flex flex-col gap-2 text-sm">
								<div class="flex justify-between">
									<span>{m.settings_version()}</span>
									<span class="font-mono">{appVersion}</span>
								</div>
								<div class="flex justify-between">
									<span>{m.settings_environment()}</span>
									<span class="font-mono">{buildType}</span>
								</div>
								<div class="flex justify-between">
									<span>{m.settings_build_date()}</span>
									<span class="font-mono">{buildDate}</span>
								</div>
								<div class="flex justify-between">
									<span>{m.settings_os_arch()}</span>
									<span class="font-mono">{architecture}-{osName}</span>
								</div>
							</div>
							<div class="pt-2 flex justify-start">
								<button
									type="button"
									class="btn btn-accent btn-sm"
									onclick={() => updaterStore.checkForUpdates(false)}
								>
									{m.settings_check_for_updates()}
								</button>
							</div>
						</div>
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>
