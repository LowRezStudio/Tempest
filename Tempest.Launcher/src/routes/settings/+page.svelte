<script lang="ts">
	import { goto } from "$app/navigation";
	import { Settings } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { platform } from "@tauri-apps/plugin-os";
	import Header from "$lib/components/ui/Header.svelte";
	import FeatureFlagsTab from "$lib/dev/FeatureFlagsTab.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createAboutInfoQuery } from "$lib/queries/about";
	import KeybindsTab from "$lib/settings/KeybindsTab.svelte";
	import {
		customThemeCSS,
		defaultInstancePath,
		modDeveloper,
		onboardingCompleted,
		theme,
		username,
	} from "$lib/stores/settings.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import WineSettings from "$lib/wine/WineSettings.svelte";

	let activeTab = $state<"general" | "keybinds" | "wine" | "advanced" | "flags">("general");

	const buildType = import.meta.env.DEV ? "Development" : "Production";

	async function browsePath() {
		const selected = await open({
			directory: true,
			multiple: false,
			title: m.settings_select_instance_folder(),
		});

		if (selected) {
			defaultInstancePath.value = selected;
		}
	}

	function resetAll() {
		localStorage.clear();
		location.href = "/";
	}

	function restartOnboarding() {
		onboardingCompleted.value = false;
		void goto("/onboarding");
	}

	const aboutQuery = createAboutInfoQuery();

	let appVersion = $derived(aboutQuery.data?.appVersion ?? "...");
	let osName = $derived(aboutQuery.data?.osName ?? "...");
	let architecture = $derived(aboutQuery.data?.architecture ?? "...");
	let buildDate = $derived(aboutQuery.data?.buildDate ?? "...");

	const isWindows = platform() === "windows";
	const tabs = $derived([
		{ name: m.settings_general(), value: "general" as const },
		{ name: m.settings_keybinds(), value: "keybinds" as const },
		...(isWindows ? [] : [{ name: m.settings_wine(), value: "wine" as const }]),
		{ name: m.settings_advanced(), value: "advanced" as const },
		...(import.meta.env.DEV
			? [{ name: m.settings_feature_flags(), value: "flags" as const }]
			: []),
	]);
</script>

<div class="bg-base-100 flex h-full flex-col">
	<Header title={m.settings_title()} {tabs} {activeTab} onSelectTab={(tab) => (activeTab = tab)}>
		{#snippet icon()}
			<Settings size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>{m.settings_configure_preferences()}</span>
		{/snippet}
	</Header>

	<div class="bg-base-100 flex flex-1 flex-col overflow-hidden">
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
								bind:value={username.value}
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
								bind:value={theme.value}
							>
								<option value="system">{m.settings_theme_system()}</option>
								<option value="mocha">{m.settings_theme_mocha()}</option>
								<option value="latte">{m.settings_theme_latte()}</option>
								<option value="legacy">{m.settings_theme_legacy()}</option>
								<option value="custom">{m.settings_theme_custom()}</option>
							</select>
						</div>

						{#if theme.value === "custom"}
							<div class="form-control">
								<label for="custom-css-input" class="label py-0.5">
									<span class="label-text text-sm"
										>{m.settings_theme_custom_css()}</span
									>
								</label>
								<textarea
									id="custom-css-input"
									class="textarea textarea-bordered w-full font-mono text-xs"
									rows="20"
									bind:value={customThemeCSS.value}
									placeholder="Paste a daisyUI @plugin &quot;daisyui/theme&quot; block..."
								></textarea>
							</div>
							<div class="bg-base-200 rounded-box mt-2 px-3 py-2">
								<p class="text-xs">
									<a
										href="https://daisyui.com/theme-generator/"
										target="_blank"
										rel="noreferrer"
										class="no-underline hover:underline"
										>daisyui.com/theme-generator</a
									>
									{m.settings_theme_custom_themes_link()}
								</p>
							</div>
						{/if}

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
									bind:value={defaultInstancePath.value}
									placeholder={m.settings_select_directory()}
								/>
								<button
									type="button"
									class="btn btn-accent join-item"
									onclick={browsePath}>{m.common_browse()}</button
								>
							</div>
						</div>

						<div class="form-control">
							<label class="label cursor-pointer justify-start gap-3 py-0.5">
								<input
									type="checkbox"
									class="toggle toggle-accent"
									bind:checked={modDeveloper.value}
								/>
								<span class="label-text text-sm">{m.settings_mod_developer()}</span>
							</label>
							<p class="mt-1 text-xs opacity-60">
								{m.settings_mod_developer_description()}
							</p>
						</div>
					</div>
				{:else if activeTab === "wine"}
					<WineSettings />
				{:else if activeTab === "keybinds"}
					<KeybindsTab />
				{:else if activeTab === "flags" && import.meta.env.DEV}
					<FeatureFlagsTab />
				{:else if activeTab === "advanced"}
					<div class="flex flex-col">
						<div class="flex flex-col gap-4">
							<h2 class="text-xl font-semibold">{m.settings_onboarding_title()}</h2>
							<p class="text-sm">
								{m.settings_onboarding_description()}
							</p>
							<div>
								<button class="btn btn-accent" onclick={restartOnboarding}
									>{m.settings_restart_onboarding()}</button
								>
							</div>
						</div>

						<div class="divider"></div>

						<div class="flex flex-col gap-4">
							<h2 class="text-error text-xl font-semibold">
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
							<div class="flex justify-start pt-2">
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
