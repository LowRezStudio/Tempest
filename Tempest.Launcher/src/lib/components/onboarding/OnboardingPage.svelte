<script lang="ts">
	import { goto } from "$app/navigation";
	import { Check, CloudDownload, Compass, FolderOpen, Loader2 } from "@lucide/svelte";
	import {
		argsFor,
		findExistingInstance,
		FolderImport,
		type LoginMethod,
	} from "$lib/components/library/folder-import.svelte";
	import FolderImportPanel from "$lib/components/library/FolderImportPanel.svelte";
	import LoginMethodPicker from "$lib/components/library/LoginMethodPicker.svelte";
	import PatchPicker from "$lib/components/library/PatchPicker.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import versions from "$lib/data/versions.json";
	import { m } from "$lib/paraglide/messages";
	import { locales, type Locale } from "$lib/paraglide/runtime";
	import {
		invoke,
		open as openDirectoryDialog,
		platform as getPlatform,
		join as sysPathJoin,
	} from "$lib/platform/api";
	import { RIGBY_BASE_URL, RIGBY_MANIFEST_URL_TEMPLATE } from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { addInstance, updateInstance } from "$lib/stores/instance.svelte";
	import { localeLabels, localeState } from "$lib/stores/locale.svelte";
	import {
		defaultInstancePath,
		onboardingCompleted,
		username,
	} from "$lib/stores/settings.svelte";
	import { downloadablePatches, type Patch } from "$lib/utils/versions";
	import WineSettings from "$lib/wine/WineSettings.svelte";
	import type { Instance } from "$lib/types/instance";

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
	let importedPaths = $state<string[]>([]);
	let downloadBusy = $state(false);
	let queuedVersions = $state<string[]>([]);
	let downloadId = $state(multiplayerVersion?.id ?? "");
	let downloadLogin = $state<LoginMethod>();
	let installError = $state("");

	// Scans from the start, so what it finds is listed by the time the import step comes up.
	const folderImport = new FolderImport(() => true);
	const currentStep = $derived(steps[stepIndex]);
	const downloadQueued = $derived(queuedVersions.length > 0);
	const downloadPatch = $derived(versions.find((entry) => entry.id === downloadId));
	const downloadNeedsLogin = $derived(downloadPatch?.version === "8.1");
	const downloadPatchQueued = $derived(
		!!downloadPatch && queuedVersions.includes(downloadPatch.version),
	);
	// Prompt text for the current step, shown as the header subtitle.
	const stepSubtitle = $derived.by(() => {
		switch (currentStep.id) {
			case "language": {
				return m.onboarding_language_title();
			}
			case "username": {
				return m.onboarding_username_title();
			}
			case "steam": {
				return m.onboarding_header_steam();
			}
			case "download": {
				return m.onboarding_header_download();
			}
			case "done": {
				return m.onboarding_done_title();
			}
			default: {
				return currentStep.label;
			}
		}
	});
	const canGoNext = $derived(currentStep.id !== "username" || usernameDraft.trim().length > 0);
	// Optional steps read "Skip" instead of "Next" until their action is used.
	const showSkipLabel = $derived(
		(currentStep.id === "steam" && importedPaths.length === 0) ||
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

	function importFolders() {
		importedPaths.push(...folderImport.import().map((instance) => instance.path));
	}

	function getDownloadBasePath(): Promise<string> {
		if (defaultInstancePath.value) return Promise.resolve(defaultInstancePath.value);
		return invoke("app:default-instance-path");
	}
	async function browseDownloadDirectory() {
		const selected = await openDirectoryDialog({
			directory: true,
			multiple: false,
			title: m.wizard_select_installation_folder(),
		});
		if (typeof selected === "string") defaultInstancePath.value = selected;
	}

	async function downloadVersion(entry: Patch) {
		if (downloadBusy || queuedVersions.includes(entry.version)) return;
		downloadBusy = true;
		installError = "";

		try {
			const targetPath = await sysPathJoin(await getDownloadBasePath(), entry.version);
			const manifests = [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", entry.id)];
			const existing = findExistingInstance(targetPath);

			if (existing) {
				restoreQueue.add({
					manifests,
					outDir: existing.path,
					baseUrl: RIGBY_BASE_URL,
					repair: true,
				});
				updateInstance(existing.id, { state: { type: "downloading" } });
			} else {
				const instance: Instance = {
					id: crypto.randomUUID(),
					label:
						entry === multiplayerVersion ? `${entry.name} (Multiplayer)` : entry.name,
					version: entry.version,
					manifestId: entry.id,
					appId: entry.appId,
					path: targetPath,
					launchOptions: {
						dllList: [],
						args: argsFor(entry.version, downloadLogin),
						noDefaultArgs: false,
						log: false,
					},
					state: { type: "downloading" },
				};
				addInstance(instance);
				restoreQueue.add({
					manifests,
					outDir: targetPath,
					baseUrl: RIGBY_BASE_URL,
				});
			}

			queuedVersions.push(entry.version);
		} catch (error) {
			console.error("[onboarding] download failed", error);
			installError = m.onboarding_install_error();
		} finally {
			downloadBusy = false;
		}
	}
</script>

<div class="relative h-screen w-full overflow-hidden">
	<div class="absolute inset-0">
		<img
			src="/loading-screens/Loading_ShootingGallery.avif"
			alt=""
			class="h-full w-full scale-110 object-cover blur-[7px]"
		/>
		<div class="absolute inset-0 bg-black/50"></div>
	</div>

	<div class="relative z-10 flex h-full flex-col">
		<button type="button" class="btn btn-ghost absolute top-6 right-4 z-20" onclick={finish}>
			{m.onboarding_skip_all()}
		</button>
		{#key localeState.current}
			<Header title={m.onboarding_title()}>
				{#snippet icon()}
					<Compass size={32} />
				{/snippet}
				{#snippet subtitle()}
					<span>{stepSubtitle}</span>
				{/snippet}
			</Header>

			<!-- my-auto rather than items-center: a card taller than the window scrolls from its top -->
			<div class="flex min-h-0 flex-1 justify-center overflow-y-auto p-6">
				<form
					class="card bg-base-200/90 my-auto w-full max-w-xl shadow-2xl backdrop-blur-md"
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
												class:btn-primary={loc === languageDraft}
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
								<div class="flex flex-col gap-4">
									<FolderImportPanel importer={folderImport} />
									<button
										class="btn btn-primary w-full"
										type="button"
										disabled={!folderImport.ready}
										onclick={importFolders}
									>
										{#if folderImport.detecting}
											<Loader2 size={16} class="animate-spin" />
											{m.common_identifying()}
										{:else if importedPaths.length > 0 && folderImport.chosen.length === 0}
											<Check size={16} />
											{m.onboarding_imported()}
										{:else}
											<FolderOpen size={16} />
											{folderImport.bulk
												? m.wizard_import_count({
														count: folderImport.chosen.length,
													})
												: m.wizard_import_instance()}
										{/if}
									</button>
								</div>
							{:else if currentStep.id === "download"}
								<div class="flex flex-col gap-4">
									<div class="flex flex-col gap-2">
										<PatchPicker
											patches={downloadablePatches}
											bind:value={downloadId}
											highlight={multiplayerVersion?.id}
										/>
										{#if multiplayerVersion}
											<p
												class="text-base-content/60 flex items-center gap-1.5 text-xs"
											>
												<span class="bg-warning h-3 w-1 shrink-0 rounded-xs"
												></span>
												{m.onboarding_multiplayer_legend({
													version: multiplayerVersion.version,
												})}
											</p>
										{/if}
									</div>

									{#if downloadNeedsLogin}
										<LoginMethodPicker bind:value={downloadLogin} />
									{/if}

									<div class="flex flex-col gap-1">
										<label
											for="onboarding-download-path"
											class="text-base-content/60 text-xs"
											>{m.onboarding_download_directory_label()}</label
										>
										<div class="join w-full">
											<input
												id="onboarding-download-path"
												type="text"
												class="input input-sm join-item min-w-0 flex-1 font-mono"
												spellcheck="false"
												bind:value={defaultInstancePath.value}
											/>
											<button
												type="button"
												class="btn btn-sm btn-square join-item"
												aria-label={m.common_browse()}
												title={m.common_browse()}
												onclick={browseDownloadDirectory}
											>
												<FolderOpen size={14} />
											</button>
										</div>
									</div>

									<button
										class="btn btn-primary w-full"
										type="button"
										disabled={!downloadPatch ||
											downloadBusy ||
											downloadPatchQueued ||
											(downloadNeedsLogin && !downloadLogin)}
										onclick={() =>
											downloadPatch && downloadVersion(downloadPatch)}
									>
										{#if downloadBusy}
											<Loader2 size={16} class="animate-spin" />
											{m.onboarding_installing()}
										{:else if downloadPatchQueued}
											<Check size={16} />
											{m.onboarding_install_queued()}
										{:else}
											<CloudDownload size={16} />
											{m.onboarding_download_version({
												version: downloadPatch?.version ?? "",
											})}
										{/if}
									</button>

									{#if installError}
										<div class="alert alert-error text-sm" role="alert">
											{installError}
										</div>
									{/if}
								</div>
							{:else if currentStep.id === "proton"}
								<WineSettings />
							{:else}
								<div class="flex flex-col items-center gap-4 py-2 text-center">
									<div
										class="btn btn-primary btn-circle pointer-events-none h-16 w-16"
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
												{#if importedPaths.length > 0}
													<span
														class="flex items-center gap-1 font-mono text-xs"
													>
														<Check size={14} class="text-success" />
														{importedPaths.join(", ")}
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
													{m.onboarding_summary_download({
														versions: queuedVersions.join(", "),
													})}
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
								<button class="btn btn-primary" type="button" onclick={finish}>
									{m.onboarding_finish()}
								</button>
							{:else}
								<button class="btn btn-primary" type="submit" disabled={!canGoNext}>
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
