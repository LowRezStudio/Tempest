<script lang="ts">
	import { Settings } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import { invalidateAll } from "$app/navigation";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createAboutInfoQuery } from "$lib/queries/about";
	import { defaultInstancePath, username } from "$lib/stores/settings";

	let activeTab = $state<"general" | "advanced">("general");

	let localUsername = $state($username);
	let localPath = $state($defaultInstancePath || "");

	let buildType = $state<string>(import.meta.env.DEV ? "Development" : "Production");

	let showSaveToast = $state(false);

	async function saveSettings() {
		username.set(localUsername);
		defaultInstancePath.set(localPath);
		await invalidateAll();

		showSaveToast = true;
		setTimeout(() => {
			showSaveToast = false;
		}, 1000);
	}

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

	function resetAll() {
		localStorage.clear();
		location.href = "/";
	}

	const aboutQuery = createAboutInfoQuery();

	let appVersion = $derived(aboutQuery.data?.appVersion ?? "...");
	let osName = $derived(aboutQuery.data?.osName ?? "...");
	let architecture = $derived(aboutQuery.data?.architecture ?? "...");
	let buildDate = $derived(aboutQuery.data?.buildDate ?? "...");
</script>

<div class="flex flex-col h-full bg-base-100">
	<Header
		title={m.settings_title()}
		tabs={[
			{ name: m.settings_general(), value: "general" },
			{ name: m.settings_advanced(), value: "advanced" },
		]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
	>
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
								<button class="btn btn-accent join-item" onclick={browsePath}
									>{m.common_browse()}</button
								>
							</div>
						</div>

						<div class="flex justify-end gap-2 pt-2">
							<button class="btn btn-accent" onclick={saveSettings}
								>{m.common_save_changes()}</button
							>
						</div>
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
						</div>
					</div>
				{/if}
			</div>
		</div>
	</div>

	<!-- Save Toast Notification -->
	{#if showSaveToast}
		<div class="toast toast-end toast-top">
			<div class="alert alert-success">
				<span>{m.settings_saved_successfully()}</span>
			</div>
		</div>
	{/if}
</div>
