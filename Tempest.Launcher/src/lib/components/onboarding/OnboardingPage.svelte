<script lang="ts">
	import { goto } from "$app/navigation";
	import { Check, Compass, Download, FolderOpen, Gamepad2, Loader2 } from "@lucide/svelte";
	import { path as tauriPath } from "@tauri-apps/api";
	import { homeDir } from "@tauri-apps/api/path";
	import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
	import { platform as getPlatform } from "@tauri-apps/plugin-os";
	import { onMount } from "svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { identifyBuild } from "$lib/core/build";
	import versions from "$lib/data/versions.json";
	import { m } from "$lib/paraglide/messages";
	import { locales, type Locale } from "$lib/paraglide/runtime";
	import { findSteamPaladinsInstallation } from "$lib/platforms/steam";
	import { createSetupInstanceMutation } from "$lib/queries/instance";
	import { RIGBY_BASE_URL, RIGBY_MANIFEST_URL_TEMPLATE } from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import {
		addInstance,
		instanceMap,
		removeInstance,
		updateInstance,
	} from "$lib/stores/instance.svelte";
	import { localeLabels, localeState } from "$lib/stores/locale.svelte";
	import {
		defaultInstancePath,
		onboardingCompleted,
		username,
	} from "$lib/stores/settings.svelte";
	import WineSettings from "$lib/wine/WineSettings.svelte";
	import type { Instance } from "$lib/types/instance";

	type SteamState = "searching" | "found" | "missing";
	type LoginMethod = "steam" | "epic" | "hirez";
	type StepId = "language" | "username" | "steam" | "download" | "proton" | "done";

	const multiplayerVersion = versions.find((version) => version.version === "0.57");

	const isLinux = getPlatform() === "linux";
	// Derived so the step labels re-translate when the locale is committed on Next.
	const steps: { id: StepId; label: string }[] = $derived.by(() => {
		void localeState.current;
		return [
			{ id: "language", label: m.onboarding_step_language() },
			{ id: "username", label: m.onboarding_step_username() },
			{ id: "steam", label: m.onboarding_step_steam() },
			{ id: "download", label: m.onboarding_step_download() },
			...(isLinux ? [{ id: "proton" as StepId, label: m.onboarding_step_proton() }] : []),
			{ id: "done", label: m.onboarding_step_done() },
		];
	});

	let stepIndex = $state(0);
	let usernameDraft = $state(username.value === "Player" ? "" : username.value);
	// Selected language is kept as a draft and only committed to the app when
	// the user confirms the step, avoiding an abrupt mid-step relayout.
	let languageDraft = $state<Locale>(localeState.current);
	let importBusy = $state(false);
	let importDone = $state(false);
	let importedPath = $state<string | null>(null);
	let downloadBusy = $state(false);
	let downloadQueued = $state(false);
	let importError = $state("");
	let installError = $state("");
	let steamState = $state<SteamState>("searching");
	let steamPath = $state<string | null>(null);
	let loginMethod = $state<LoginMethod | undefined>();

	const setupInstanceMutation = createSetupInstanceMutation();
	const currentStep = $derived(steps[stepIndex]);
	// Prompt text for the current step, shown as the header subtitle.
	const stepSubtitle = $derived.by(() => {
		switch (currentStep.id) {
			case "language":
				return m.onboarding_language_title();
			case "username":
				return m.onboarding_username_title();
			case "steam":
				return m.onboarding_header_steam();
			case "download":
				return m.onboarding_header_download();
			case "done":
				return m.onboarding_done_title();
			default:
				return currentStep.label;
		}
	});
	const canGoNext = $derived(currentStep.id !== "username" || usernameDraft.trim().length > 0);
	// Optional steps read "Skip" instead of "Next" until their action is used.
	const showSkipLabel = $derived(
		(currentStep.id === "steam" && !importBusy && !importDone) ||
			(currentStep.id === "download" && !downloadBusy && !downloadQueued),
	);

	let stepContent: HTMLDivElement | undefined = $state();

	// Focus the first focusable element whenever the step changes (or after
	// the locale-driven remount of the wizard content).
	$effect(() => {
		const stepId = currentStep.id;
		void localeState.current;
		queueMicrotask(() => {
			const stepRoot = stepContent;
			if (!stepRoot) return;

			if (stepId === "language") {
				stepRoot.querySelector<HTMLElement>(`[data-locale="${languageDraft}"]`)?.focus();
				return;
			}

			// Never steal focus on the final screen.
			if (stepId === "done") return;

			stepRoot
				.querySelector<HTMLElement>(
					"input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled])",
				)
				?.focus();
		});
	});

	// Enter continues to the next step (or finishes on the last one).
	// Text inputs submit the wizard form instead; buttons keep their native Enter behavior.
	$effect(() => {
		const handleKeydown = (event: KeyboardEvent) => {
			if (event.key !== "Enter") return;
			const target = event.target as Element | null;

			if (currentStep.id === "done") {
				if (target?.closest("input, textarea, button, select")) return;
				event.preventDefault();
				finish();
				return;
			}

			// Language buttons are pure selectors — Enter always continues here.
			if (currentStep.id === "language") {
				event.preventDefault();
				goNext();
				return;
			}

			if (target?.closest("input, textarea, button, select")) return;
			event.preventDefault();
			goNext();
		};

		window.addEventListener("keydown", handleKeydown);
		return () => window.removeEventListener("keydown", handleKeydown);
	});

	onMount(() => {
		void discoverSteam();
	});

	async function discoverSteam() {
		try {
			steamPath = await findSteamPaladinsInstallation();
			steamState = steamPath ? "found" : "missing";
		} catch (error) {
			console.debug("[onboarding] Steam detection failed", error);
			steamState = "missing";
		}
	}

	function saveUsername(): boolean {
		const value = usernameDraft.trim();
		if (!value) return false;

		username.value = value;
		return true;
	}

	function goNext() {
		if (!canGoNext) return;
		if (currentStep.id === "language") {
			localeState.set(languageDraft);
		} else if (currentStep.id === "username") saveUsername();
		stepIndex = Math.min(stepIndex + 1, steps.length - 1);
	}

	function goBack() {
		stepIndex = Math.max(stepIndex - 1, 0);
	}

	function finish() {
		// Flips the root layout flag and returns to the home page.
		onboardingCompleted.value = true;
		void goto("/");
	}

	function normalizedPath(value: string): string {
		return value
			.trim()
			.replace(/[\\/]+$/, "")
			.toLowerCase();
	}

	function findExistingInstance(targetPath: string): Instance | undefined {
		const target = normalizedPath(targetPath);
		return Object.values(instanceMap.value).find(
			(instance) => instance?.path && normalizedPath(instance.path) === target,
		);
	}

	async function prepareImportedInstance(instance: Instance): Promise<boolean> {
		try {
			await setupInstanceMutation.mutateAsync(instance);
			return true;
		} catch (error) {
			console.error("[onboarding] instance setup failed", error);
			return false;
		} finally {
			updateInstance(instance.id, { state: { type: "prepared" } });
		}
	}

	async function importInstallation(targetPath: string, label: string, args: string[] = []) {
		if (importBusy || importDone) return;
		importBusy = true;
		importError = "";

		try {
			const build = await identifyBuild(targetPath);
			const version =
				versions.find((entry) => entry.id === build?.Id) ??
				versions.find((entry) => entry.version === build?.VersionGroup);
			const existing = findExistingInstance(targetPath);

			if (existing) {
				importedPath = targetPath;
				importDone = true;
				return;
			}

			const instance: Instance = {
				id: crypto.randomUUID(),
				label: version?.name ?? label,
				version: version?.version ?? build?.VersionGroup,
				manifestId: version?.id ?? build?.Id,
				appId: version?.appId ?? 444090,
				path: targetPath,
				launchOptions: {
					dllList: [],
					args,
					noDefaultArgs: false,
					log: false,
				},
				state: { type: "setup" },
			};

			addInstance(instance);
			if (!(await prepareImportedInstance(instance))) {
				removeInstance(instance.id);
				importError = m.onboarding_import_error();
				return;
			}

			importedPath = targetPath;
			importDone = true;
		} catch (error) {
			console.error("[onboarding] import failed", error);
			importError = m.onboarding_import_error();
		} finally {
			importBusy = false;
		}
	}

	async function importSteamInstallation() {
		if (!steamPath || !loginMethod) return;
		const args = loginMethod === "steam" ? ["-steam"] : loginMethod === "epic" ? ["-epic"] : [];
		await importInstallation(steamPath, "Paladins (Steam)", args);
	}

	async function importAnotherInstallation() {
		if (importBusy || importDone) return;

		const selected = await openDirectoryDialog({
			directory: true,
			multiple: false,
			title: m.wizard_select_installation_folder(),
		});
		if (typeof selected === "string") await importInstallation(selected, "Paladins");
	}

	async function getDownloadBasePath(): Promise<string> {
		if (defaultInstancePath.value) return defaultInstancePath.value;
		const root = getPlatform() === "windows" ? "C:" : await homeDir();
		return await tauriPath.join(root, "Games", "Tempest");
	}

	async function installMultiplayerVersion() {
		if (downloadBusy || downloadQueued || !multiplayerVersion) return;
		downloadBusy = true;
		installError = "";

		try {
			const targetPath = await tauriPath.join(
				await getDownloadBasePath(),
				multiplayerVersion.version,
			);
			const existing = findExistingInstance(targetPath);
			if (existing) {
				restoreQueue.add({
					manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", multiplayerVersion.id)],
					outDir: existing.path,
					baseUrl: RIGBY_BASE_URL,
				});
				updateInstance(existing.id, { state: { type: "downloading" } });
			} else {
				const instance: Instance = {
					id: crypto.randomUUID(),
					label: `${multiplayerVersion.name} (Multiplayer)`,
					version: multiplayerVersion.version,
					manifestId: multiplayerVersion.id,
					appId: multiplayerVersion.appId,
					path: targetPath,
					launchOptions: {
						dllList: [],
						args: [],
						noDefaultArgs: false,
						log: false,
					},
					state: { type: "downloading" },
				};
				addInstance(instance);
				restoreQueue.add({
					manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", multiplayerVersion.id)],
					outDir: targetPath,
					baseUrl: RIGBY_BASE_URL,
				});
			}

			downloadQueued = true;
		} catch (error) {
			console.error("[onboarding] multiplayer install failed", error);
			installError = m.onboarding_install_error();
		} finally {
			downloadBusy = false;
		}
	}
</script>

<div class="relative h-screen w-full overflow-hidden">
	<div class="absolute inset-0">
		<img
			src="/loading-screens/Loading_ShootingGallery.webp"
			alt=""
			class="h-full w-full scale-110 object-cover blur-[7px]"
		/>
		<div class="absolute inset-0 bg-black/50"></div>
	</div>

	<div class="relative z-10 flex h-full flex-col">
		{#key localeState.current}
			<Header title={m.onboarding_title()}>
				{#snippet icon()}
					<Compass size={32} />
				{/snippet}
				{#snippet subtitle()}
					<span>{stepSubtitle}</span>
				{/snippet}
			</Header>

			<div class="flex flex-1 items-center justify-center p-6">
				<form
					class="card bg-base-200/90 w-full max-w-xl shadow-2xl backdrop-blur-md"
					onsubmit={(event) => {
						event.preventDefault();
						goNext();
					}}
				>
					<div class="card-body gap-4">
						<h1 class="text-xl font-bold uppercase">{stepSubtitle}</h1>

						<ul class="steps w-full text-xs">
							{#each steps as step, index}
								<li
									class="step relative"
									class:step-primary={index <= stepIndex}
									data-content={index + 1}
								>
									{#if index < stepIndex}
										<button
											type="button"
											class="relative z-10 col-start-1 row-start-1 h-8 w-8 cursor-pointer place-self-center rounded-full"
											aria-label={step.label}
											onclick={() => (stepIndex = index)}
										></button>
									{/if}
									{step.label}
								</li>
							{/each}
						</ul>

						<div bind:this={stepContent}>
							{#if currentStep.id === "language"}
								<div class="form-control">
									<div class="grid grid-cols-2 gap-2">
										{#each locales as loc}
											<button
												class="btn h-12 justify-start gap-3"
												class:btn-accent={loc === languageDraft}
												class:btn-ghost={loc !== languageDraft}
												type="button"
												data-locale={loc}
												onclick={() => (languageDraft = loc)}
											>
												<span class="text-base"
													>{localeLabels[loc]?.flag ?? ""}</span
												>
												<span class="text-sm font-semibold">
													{localeLabels[loc]?.label ?? loc.toUpperCase()}
												</span>
											</button>
										{/each}
									</div>
								</div>
							{:else if currentStep.id === "username"}
								<div class="form-control">
									<input
										id="onboarding-username"
										type="text"
										class="input input-bordered w-full"
										class:input-error={!canGoNext}
										aria-label={m.onboarding_username_title()}
										bind:value={usernameDraft}
										onblur={saveUsername}
										placeholder={m.onboarding_username_placeholder()}
										autocomplete="nickname"
									/>
									{#if !canGoNext}
										<div class="label py-1">
											<span class="label-text-alt text-error"
												>{m.onboarding_username_required()}</span
											>
										</div>
									{/if}
								</div>
							{:else if currentStep.id === "steam"}
								{#if steamState === "searching"}
									<div class="alert">
										<Loader2 size={16} class="animate-spin" />
										<span>{m.onboarding_steam_searching()}</span>
									</div>
								{:else if steamState === "found" && steamPath}
									<div class="alert">
										<Gamepad2 size={18} class="mt-0.5 shrink-0" />
										<div class="min-w-0">
											<h4 class="text-sm font-semibold">
												{m.onboarding_steam_found()}
											</h4>
											<p class="mt-1 font-mono text-xs break-all opacity-60">
												{steamPath}
											</p>
										</div>
									</div>

									<div class="bg-base-300/30 rounded-box space-y-3 p-4">
										<div>
											<h4 class="text-sm font-semibold">
												{m.onboarding_login_title()}
											</h4>
										</div>
										<div class="flex flex-col gap-2">
											<button
												class="btn h-12 justify-start gap-3"
												class:btn-accent={loginMethod === "steam"}
												class:btn-ghost={loginMethod !== "steam"}
												type="button"
												onclick={() => (loginMethod = "steam")}
											>
												<div class="text-left">
													<div class="text-sm font-semibold">
														{m.wizard_login_steam()}
													</div>
													<div class="text-xs opacity-60">
														{m.wizard_login_steam_desc()}
													</div>
												</div>
											</button>
											<button
												class="btn h-12 justify-start gap-3"
												class:btn-accent={loginMethod === "epic"}
												class:btn-ghost={loginMethod !== "epic"}
												type="button"
												onclick={() => (loginMethod = "epic")}
											>
												<div class="text-left">
													<div class="text-sm font-semibold">
														{m.wizard_login_epic()}
													</div>
													<div class="text-xs opacity-60">
														{m.wizard_login_epic_desc()}
													</div>
												</div>
											</button>
											<button
												class="btn h-12 justify-start gap-3"
												class:btn-accent={loginMethod === "hirez"}
												class:btn-ghost={loginMethod !== "hirez"}
												type="button"
												onclick={() => (loginMethod = "hirez")}
											>
												<div class="text-left">
													<div class="text-sm font-semibold">
														{m.wizard_login_hirez()}
													</div>
													<div class="text-xs opacity-60">
														{m.wizard_login_hirez_desc()}
													</div>
												</div>
											</button>
										</div>

										<button
											class="btn btn-accent w-full"
											type="button"
											disabled={importBusy || importDone || !loginMethod}
											onclick={importSteamInstallation}
										>
											{#if importBusy}
												<Loader2 size={16} class="animate-spin" />
											{:else if importDone}
												<Check size={16} />
											{:else}
												<FolderOpen size={16} />
											{/if}
											{importDone
												? m.onboarding_imported()
												: m.onboarding_import_steam()}
										</button>
									</div>
								{:else}
									<div class="alert alert-info text-sm">
										<Gamepad2 size={16} />
										<span>{m.onboarding_steam_not_found()}</span>
									</div>
								{/if}

								<div class="divider my-2"></div>
								<button
									class="btn btn-ghost w-full justify-start gap-3"
									type="button"
									disabled={importBusy || importDone}
									onclick={importAnotherInstallation}
								>
									<FolderOpen size={16} />
									{m.onboarding_import_another()}
								</button>

								{#if importError}
									<div class="alert alert-error text-sm" role="alert">
										{importError}
									</div>
								{/if}
							{:else if currentStep.id === "download"}
								<button
									class="btn btn-accent h-14 w-full justify-start gap-3"
									type="button"
									disabled={downloadBusy || downloadQueued || !multiplayerVersion}
									onclick={installMultiplayerVersion}
								>
									{#if downloadBusy}
										<Loader2 size={16} class="animate-spin" />
									{:else if downloadQueued}
										<Check size={16} />
									{:else}
										<Download size={16} />
									{/if}
									<span class="text-left">
										<span class="block text-sm font-semibold">
											{downloadQueued
												? m.onboarding_install_queued()
												: m.onboarding_download_title()}
										</span>
										<span class="block text-xs opacity-70">
											{downloadBusy
												? m.onboarding_installing()
												: m.onboarding_download_description()}
										</span>
									</span>
								</button>

								{#if installError}
									<div class="alert alert-error text-sm" role="alert">
										{installError}
									</div>
								{/if}
							{:else if currentStep.id === "proton"}
								<WineSettings />
							{:else}
								<div class="flex flex-col items-center gap-4 py-2 text-center">
									<div
										class="btn btn-accent btn-circle pointer-events-none h-16 w-16"
									>
										<Check size={32} />
									</div>
									<div>
										<p class="mt-1 text-sm opacity-70">
											{m.onboarding_done_description()}
										</p>
									</div>
									<div class="w-full space-y-2 text-left">
										<div class="flex items-center gap-2 text-sm">
											<span class="text-base-content/50 w-28 shrink-0"
												>{m.onboarding_step_language()}</span
											>
											<span>
												{localeLabels[localeState.current]?.flag ?? ""}
												{localeLabels[localeState.current]?.label ??
													localeState.current.toUpperCase()}
											</span>
										</div>
										<div class="flex items-center gap-2 text-sm">
											<span class="text-base-content/50 w-28 shrink-0"
												>{m.onboarding_step_username()}</span
											>
											<span class="font-semibold">{username.value}</span>
										</div>
										<div class="flex items-center gap-2 text-sm">
											<span class="text-base-content/50 w-28 shrink-0"
												>{m.onboarding_step_steam()}</span
											>
											<span class="min-w-0 flex-1 truncate">
												{#if importedPath}
													<span
														class="flex items-center gap-1 font-mono text-xs"
													>
														<Check size={14} class="text-success" />
														{importedPath}
													</span>
												{:else}
													<span class="opacity-70"
														>{m.onboarding_summary_none()}</span
													>
												{/if}
											</span>
										</div>
										<div class="flex items-center gap-2 text-sm">
											<span class="text-base-content/50 w-28 shrink-0"
												>{m.onboarding_step_download()}</span
											>
											{#if downloadQueued}
												<span class="flex items-center gap-1">
													<Check size={14} class="text-success" />
													{m.onboarding_summary_download()}
												</span>
											{:else}
												<span class="opacity-70"
													>{m.onboarding_summary_none()}</span
												>
											{/if}
										</div>
									</div>
								</div>
							{/if}
						</div>

						<div class="mt-2 flex w-full items-center justify-end gap-2">
							{#if stepIndex > 0}
								<button class="btn btn-ghost" type="button" onclick={goBack}>
									{m.onboarding_back()}
								</button>
							{/if}
							{#if currentStep.id === "done"}
								<button class="btn btn-accent" type="button" onclick={finish}>
									{m.onboarding_finish()}
								</button>
							{:else}
								<button class="btn btn-accent" type="submit" disabled={!canGoNext}>
									{showSkipLabel ? m.onboarding_skip() : m.onboarding_next()}
								</button>
							{/if}
						</div>
					</div>
				</form>
			</div>
		{/key}
	</div>
</div>
