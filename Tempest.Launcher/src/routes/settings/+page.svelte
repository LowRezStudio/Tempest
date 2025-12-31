<script lang="ts">
	import { username, defaultInstancePath } from "$lib/stores/settings";
	import { open } from "@tauri-apps/plugin-dialog";
	import { invalidateAll } from "$app/navigation";
	import { Settings } from "@lucide/svelte";
	import { getVersion } from "@tauri-apps/api/app";
	import { arch, type as osType } from "@tauri-apps/plugin-os";

	let activeTab = $state<"general" | "advanced">("general");

	let localUsername = $state($username);
	let localPath = $state($defaultInstancePath || "");

	let appVersion = $state<string>("...");
	let osName = $state<string>("...");
	let architecture = $state<string>("...");
	let buildDate = $state<string>("...");
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
			title: "Select Default Instance Path",
		});

		if (selected) {
			localPath = selected;
		}
	}

	async function resetAll() {
		localStorage.clear();
		location.href = "/";
	}

	$effect(() => {
		async function loadAboutInfo() {
			appVersion = await getVersion();
			osName = osType();
			architecture = arch();
			buildDate = new Date(__BUILD_DATE__).toUTCString();
		}
		loadAboutInfo();
	});
</script>

<div class="flex flex-col h-full bg-base-100">
	<div>
		<div class="px-4 py-3">
			<div class="flex items-center justify-between">
				<div class="flex items-center gap-3">
					<div
						class="w-16 h-16 rounded-xl bg-base-200 flex items-center justify-center shrink-0"
					>
						<Settings size={32} class="opacity-60" />
					</div>
					<div>
						<h1 class="text-2xl font-bold mb-1">Settings</h1>
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<span>Configure your launcher preferences</span>
						</div>
					</div>
				</div>
			</div>
		</div>

		<div class="px-4">
			<div role="tablist" class="tabs tabs-border">
				<button
					role="tab"
					class={activeTab === "general" ? "tab tab-active" : "tab"}
					onclick={() => (activeTab = "general")}
				>
					General
				</button>
				<button
					role="tab"
					class={activeTab === "advanced" ? "tab tab-active" : "tab"}
					onclick={() => (activeTab = "advanced")}
				>
					Advanced
				</button>
			</div>
		</div>
	</div>

	<div class="flex-1 flex flex-col overflow-hidden bg-base-200">
		<div class="flex-1 overflow-y-auto">
			<div class="p-6">
				{#if activeTab === "general"}
					<div class="flex flex-col gap-4">
						<div class="form-control">
							<label for="username-input" class="label py-0.5">
								<span class="label-text text-sm">Username</span>
							</label>
							<input
								id="username-input"
								type="text"
								class="input input-bordered w-full"
								bind:value={localUsername}
								placeholder="Enter your username"
							/>
						</div>

						<div class="form-control">
							<label for="path-input" class="label py-0.5">
								<span class="label-text text-sm">Default Instance Path</span>
							</label>
							<div class="join w-full">
								<input
									id="path-input"
									type="text"
									class="input input-bordered join-item flex-1 font-mono"
									bind:value={localPath}
									placeholder="Select a directory..."
								/>
								<button class="btn btn-accent join-item" onclick={browsePath}
									>Browse</button
								>
							</div>
						</div>

						<div class="flex justify-end gap-2 pt-2">
							<button class="btn btn-accent" onclick={saveSettings}
								>Save Changes</button
							>
						</div>
					</div>
				{:else if activeTab === "advanced"}
					<div class="flex flex-col">
						<div class="flex flex-col gap-4">
							<h2 class="text-xl font-semibold text-error">Danger Zone</h2>
							<p class="text-sm">
								Resetting will clear all your settings and return everything to
								default values. This action cannot be undone.
							</p>
							<div>
								<button class="btn btn-error" onclick={resetAll}
									>Clear All Settings</button
								>
							</div>
						</div>

						<div class="divider"></div>

						<div class="flex flex-col gap-4">
							<h2 class="text-xl font-semibold">About</h2>
							<div class="flex flex-col gap-2 text-sm">
								<div class="flex justify-between">
									<span>Version</span>
									<span class="font-mono">{appVersion}</span>
								</div>
								<div class="flex justify-between">
									<span>Environment</span>
									<span class="font-mono">{buildType}</span>
								</div>
								<div class="flex justify-between">
									<span>Build Date</span>
									<span class="font-mono">{buildDate}</span>
								</div>
								<div class="flex justify-between">
									<span>OS & Arch</span>
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
				<span>Settings saved successfully</span>
			</div>
		</div>
	{/if}
</div>
