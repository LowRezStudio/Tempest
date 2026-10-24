<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		Compass,
		Cpu,
		ExternalLink,
		Flag,
		FlaskConical,
		FolderOpen,
		Info,
		Keyboard,
		KeyRound,
		KeySquare,
		Palette,
		RefreshCw,
		Rocket,
		RotateCcw,
		Settings,
		SlidersHorizontal,
		Snowflake,
		Terminal,
		Trash2,
		TriangleAlert,
		Wine,
	} from "@lucide/svelte";
	import ActionRow from "$lib/components/ui/ActionRow.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import Panel from "$lib/components/ui/Panel.svelte";
	import SectionNav from "$lib/components/ui/SectionNav.svelte";
	import { SectionTracker, type PageSection } from "$lib/components/ui/sections.svelte";
	import SwitchRow from "$lib/components/ui/SwitchRow.svelte";
	import FeatureFlagsSettings from "$lib/dev/FeatureFlagsSettings.svelte";
	import { m } from "$lib/paraglide/messages";
	import { locales } from "$lib/paraglide/runtime";
	import { invoke, open, openUrl, platform } from "$lib/platform/api";
	import { createAboutInfoQuery } from "$lib/queries/about";
	import ApiKeySettings from "$lib/settings/ApiKeySettings.svelte";
	import CliInstall from "$lib/settings/CliInstall.svelte";
	import KeybindsSettings from "$lib/settings/KeybindsSettings.svelte";
	import SigningKeySettings from "$lib/settings/SigningKeySettings.svelte";
	import ThemePicker from "$lib/settings/ThemePicker.svelte";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";
	import { resetKeybinds } from "$lib/stores/keybinds.svelte";
	import { localeLabels, localeState } from "$lib/stores/locale.svelte";
	import {
		customThemeCSS,
		defaultInstancePath,
		modDeveloper,
		modRepositoryURL,
		onboardingCompleted,
		openSslCapFix,
		seasonalEffects,
		theme,
		username,
	} from "$lib/stores/settings.svelte";
	import { snapshotPaused } from "$lib/stores/snapshot.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import { weatherOverride } from "$lib/theme/weather.svelte";
	import WineSettings from "$lib/wine/WineSettings.svelte";

	const DEFAULT_REPOSITORY = "https://mods.lowrezstudio.com";
	const isWindows = platform() === "windows";
	const buildType = import.meta.env.DEV ? "Development" : "Production";

	const publishingEnabled = $derived(isFlagEnabled("mod-publishing"));
	const repoBase = $derived(modRepositoryURL.value.replace(/\/+$/, ""));

	const sections: PageSection[] = $derived([
		{ id: "general", title: m.settings_general(), icon: SlidersHorizontal },
		{ id: "appearance", title: m.settings_appearance(), icon: Palette },
		{ id: "launching", title: m.settings_launching(), icon: Rocket },
		...(isWindows ? [] : [{ id: "wine", title: m.settings_wine(), icon: Wine }]),
		{ id: "keybinds", title: m.settings_keybinds(), icon: Keyboard },
		{ id: "mod-development", title: m.settings_mod_development(), icon: FlaskConical },
		...(modDeveloper.value
			? [{ id: "signing-key", title: m.settings_signing_key_label(), icon: KeyRound }]
			: []),
		...(modDeveloper.value && publishingEnabled
			? [{ id: "api-key", title: m.settings_api_key_label(), icon: KeySquare }]
			: []),
		{ id: "terminal", title: m.settings_cli_title(), icon: Terminal },
		...(import.meta.env.DEV
			? [{ id: "feature-flags", title: m.settings_feature_flags(), icon: Flag }]
			: []),
		{ id: "danger-zone", title: m.settings_danger_zone(), icon: TriangleAlert, tone: "danger" },
	]);
	const tracker = new SectionTracker(() => sections);

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

	function restartOnboarding() {
		onboardingCompleted.value = false;
		void goto("/onboarding");
	}

	let confirmKeybindsReset = $state(false);
	function resetBindings() {
		if (!confirmKeybindsReset) {
			confirmKeybindsReset = true;
			setTimeout(() => (confirmKeybindsReset = false), 3000);
			return;
		}
		confirmKeybindsReset = false;
		resetKeybinds();
	}

	let confirmClear = $state(false);
	function resetAll() {
		// Pause the layout's snapshot interval first: a tick firing between the
		// snapshot deletion and the clear() below would re-dump localStorage into
		// settings.json, and the startup migration would import everything back.
		snapshotPaused.value = true;
		// Also drop the settings.json snapshot, otherwise the startup migration
		// imports everything right back after the reload.
		void (async () => {
			try {
				await invoke("settings:delete-snapshot");
			} catch {
				/* nothing to remove */
			}
			localStorage.clear();
			location.href = "/";
		})();
	}

	const aboutQuery = createAboutInfoQuery();
	const about = $derived(aboutQuery.data);
	const builtOn = $derived(about ? new Date(about.buildDate).toLocaleDateString() : "...");
	const checkingUpdates = $derived(updaterStore.status === "checking");
</script>

{#snippet websiteLink(anchor: string)}
	<button
		type="button"
		class="btn btn-ghost btn-xs text-base-content/70"
		onclick={() => openUrl(`${repoBase}/settings#${anchor}`)}
	>
		{m.settings_manage_on_website()}
		<ExternalLink size={12} />
	</button>
{/snippet}

<div class="flex h-full flex-col">
	<Header title={m.settings_title()}>
		{#snippet icon()}
			<Settings size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>{m.settings_configure_preferences()}</span>
		{/snippet}
	</Header>

	<div
		class="@container min-h-0 flex-1 overflow-y-auto"
		onscroll={tracker.track}
		onscrollend={tracker.settle}
		onwheel={tracker.settle}
	>
		<div
			class="mx-auto grid w-full max-w-6xl items-start gap-4 px-4 py-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]"
		>
			<div class="flex min-w-0 flex-col gap-4">
				<Panel {...tracker.panel("general")}>
					<div class="grid items-start gap-3 @lg:grid-cols-2">
						<div class="flex flex-col gap-1">
							<label for="username-input" class="text-base-content/60 text-xs"
								>{m.settings_username()}</label
							>
							<input
								id="username-input"
								type="text"
								class="input input-sm w-full"
								bind:value={username.value}
								placeholder={m.settings_username_placeholder()}
							/>
						</div>
						<label class="flex flex-col gap-1">
							<span class="text-base-content/60 text-xs">{m.common_language()}</span>
							<select
								class="select select-sm w-full"
								bind:value={localeState.current}
							>
								{#each locales as loc (loc)}
									<option value={loc}
										>{localeLabels[loc]?.flag ?? ""}
										{localeLabels[loc]?.label ?? loc.toUpperCase()}</option
									>
								{/each}
							</select>
						</label>
						<div class="flex flex-col gap-1 @lg:col-span-2">
							<label for="instance-path-input" class="text-base-content/60 text-xs"
								>{m.settings_default_instance_path()}</label
							>
							<div class="join w-full">
								<input
									id="instance-path-input"
									type="text"
									class="input input-sm join-item min-w-0 flex-1 font-mono"
									spellcheck="false"
									bind:value={defaultInstancePath.value}
									placeholder={m.settings_select_directory()}
								/>
								<button
									type="button"
									class="btn btn-sm join-item"
									onclick={browsePath}
								>
									<FolderOpen size={14} />
									{m.common_browse()}
								</button>
							</div>
						</div>
					</div>
					<div class="mt-3">
						<ActionRow
							icon={Compass}
							title={m.settings_onboarding_title()}
							hint={m.settings_onboarding_description()}
						>
							<button
								type="button"
								class="btn btn-sm shrink-0"
								onclick={restartOnboarding}
							>
								{m.settings_restart_onboarding()}
							</button>
						</ActionRow>
					</div>
				</Panel>

				<Panel {...tracker.panel("appearance")}>
					<ThemePicker />
					{#if theme.value === "custom"}
						<label class="mt-3 flex flex-col gap-1">
							<span class="text-base-content/60 text-xs"
								>{m.settings_theme_custom_css()}</span
							>
							<textarea
								class="textarea textarea-sm w-full font-mono text-xs"
								rows="12"
								spellcheck="false"
								bind:value={customThemeCSS.value}
								placeholder="Paste a daisyUI @plugin &quot;daisyui/theme&quot; block..."
							></textarea>
						</label>
						<p class="text-base-content/60 mt-1 text-xs">
							<button
								type="button"
								class="link"
								onclick={() => openUrl("https://daisyui.com/theme-generator/")}
								>daisyui.com/theme-generator</button
							>
							{m.settings_theme_custom_themes_link()}
						</p>
					{/if}
					<div class="mt-3">
						<!-- Flipping it also ends a Konami code toggle, so what shows matches the switch -->
						<SwitchRow
							bind:checked={
								() => seasonalEffects.value,
								(on) => {
									seasonalEffects.value = on;
									weatherOverride.value = null;
								}
							}
							icon={Snowflake}
							title={m.settings_seasonal_effects()}
							hint={m.settings_seasonal_effects_hint()}
						/>
					</div>
				</Panel>

				<Panel {...tracker.panel("launching")}>
					<SwitchRow
						bind:checked={openSslCapFix.value}
						icon={Cpu}
						title={m.settings_open_ssl_fix()}
						hint={m.settings_open_ssl_fix_description()}
					/>
				</Panel>

				{#if !isWindows}
					<Panel {...tracker.panel("wine")}>
						<WineSettings />
					</Panel>
				{/if}

				<Panel {...tracker.panel("keybinds")}>
					{#snippet aside()}
						<button
							type="button"
							class={["btn btn-xs", confirmKeybindsReset ? "btn-error" : "btn-ghost"]}
							onclick={resetBindings}
						>
							<RotateCcw size={12} />
							{confirmKeybindsReset
								? m.settings_keybinds_reset_confirm()
								: m.settings_keybinds_reset()}
						</button>
					{/snippet}
					<KeybindsSettings />
				</Panel>

				<Panel {...tracker.panel("mod-development")}>
					<div class="flex flex-col gap-3">
						<SwitchRow
							bind:checked={modDeveloper.value}
							icon={FlaskConical}
							title={m.settings_mod_developer()}
							hint={m.settings_mod_developer_description()}
						/>
						<div class="flex flex-col gap-1">
							<label for="repo-url-input" class="text-base-content/60 text-xs"
								>{m.settings_mod_repository_url()}</label
							>
							<div class="join w-full">
								<input
									id="repo-url-input"
									type="url"
									class="input input-sm join-item min-w-0 flex-1 font-mono"
									spellcheck="false"
									bind:value={modRepositoryURL.value}
									placeholder={DEFAULT_REPOSITORY}
								/>
								<button
									type="button"
									class="btn btn-sm join-item"
									disabled={modRepositoryURL.value === DEFAULT_REPOSITORY}
									onclick={() => (modRepositoryURL.value = DEFAULT_REPOSITORY)}
								>
									{m.common_reset()}
								</button>
							</div>
						</div>
					</div>
				</Panel>

				{#if modDeveloper.value}
					<Panel {...tracker.panel("signing-key")}>
						{#snippet aside()}{@render websiteLink("signing-keys")}{/snippet}
						<SigningKeySettings />
					</Panel>
				{/if}

				{#if modDeveloper.value && publishingEnabled}
					<Panel {...tracker.panel("api-key")}>
						{#snippet aside()}{@render websiteLink("api-keys")}{/snippet}
						<ApiKeySettings />
					</Panel>
				{/if}

				<Panel {...tracker.panel("terminal")}>
					<CliInstall />
				</Panel>

				{#if import.meta.env.DEV}
					<Panel {...tracker.panel("feature-flags")}>
						<FeatureFlagsSettings />
					</Panel>
				{/if}

				<Panel {...tracker.panel("danger-zone")}>
					<p class="text-base-content/70 mb-3 text-sm">{m.settings_reset_warning()}</p>
					<button
						type="button"
						class="btn btn-error btn-soft btn-sm"
						onclick={() => (confirmClear = true)}
					>
						<Trash2 size={14} />
						{m.settings_clear_all()}…
					</button>
				</Panel>
			</div>

			<aside class="flex flex-col gap-3 @3xl:sticky @3xl:top-6">
				<div
					data-panel
					class="bg-base-200 border-base-300 rounded-box border p-4 @max-3xl:hidden"
				>
					<SectionNav {tracker} />
				</div>

				<div
					data-panel
					class="bg-base-200 border-base-300 rounded-box flex flex-col gap-3 border p-4"
				>
					<div class="flex items-center gap-2">
						<span
							class="bg-base-300 flex size-5 shrink-0 items-center justify-center rounded-full"
							><Info size={12} /></span
						>
						<h2 class="text-sm font-semibold">{m.settings_about()}</h2>
					</div>
					<dl class="grid grid-cols-2 gap-3 text-sm">
						<div class="min-w-0">
							<dt class="text-base-content/60 text-xs">{m.settings_version()}</dt>
							<dd class="truncate font-mono">{about?.appVersion ?? "..."}</dd>
						</div>
						<div class="min-w-0">
							<dt class="text-base-content/60 text-xs">{m.settings_environment()}</dt>
							<dd class="truncate">{buildType}</dd>
						</div>
						<div class="min-w-0">
							<dt class="text-base-content/60 text-xs">{m.settings_build_date()}</dt>
							<dd class="truncate" title={about?.buildDate}>{builtOn}</dd>
						</div>
						<div class="min-w-0">
							<dt class="text-base-content/60 text-xs">{m.settings_os_arch()}</dt>
							<dd class="truncate font-mono">
								{about ? `${about.architecture}-${about.osName}` : "..."}
							</dd>
						</div>
					</dl>
					<button
						type="button"
						class="btn btn-sm w-full"
						disabled={checkingUpdates}
						onclick={() => updaterStore.checkForUpdates(false)}
					>
						{#if checkingUpdates}
							<span class="loading loading-spinner loading-xs"></span>
						{:else}
							<RefreshCw size={14} />
						{/if}
						{m.settings_check_for_updates()}
					</button>
				</div>
			</aside>
		</div>
	</div>
</div>

<Modal
	bind:open={confirmClear}
	title={m.settings_clear_all_title()}
	class="max-w-sm"
	onsubmit={resetAll}
>
	<p class="text-base-content/70 text-sm">{m.settings_clear_all_confirm_text()}</p>
	{#snippet actions()}
		<button type="button" class="btn btn-ghost btn-sm" onclick={() => (confirmClear = false)}>
			{m.common_cancel()}
		</button>
		<button type="submit" class="btn btn-error btn-sm">
			<Trash2 size={14} />
			{m.settings_clear_all()}
		</button>
	{/snippet}
</Modal>
