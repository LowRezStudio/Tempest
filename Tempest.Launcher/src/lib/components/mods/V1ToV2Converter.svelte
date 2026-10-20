<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		BadgeCheck,
		CheckCircle2,
		File,
		FileUp,
		FlaskConical,
		FolderOpen,
		Image as ImageIcon,
		Plus,
		Trash2,
		Upload,
		X,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import PublishModal from "$lib/components/mods/PublishModal.svelte";
	import { installMod, type RepoPublishResult } from "$lib/core/mods";
	import { converterPendingPaths } from "$lib/mods/drop.svelte";
	import {
		MOD_ID_PATTERN,
		buildPackageZip,
		detectKind,
		optimizeUpkEntries,
		parseV1Package,
		slugify,
		targetDirFor,
	} from "$lib/mods/packaging";
	import { signWithPrompt } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openUrl } from "$lib/platform/api";
	import { converter } from "$lib/stores/converter.svelte";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { modRepositoryURL } from "$lib/stores/settings.svelte";
	import { addToast, removeToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	type SourceEntry = {
		label: string;
		files: { path: string; data: Uint8Array }[];
		readme?: string;
	};

	let cooking = $state(false);
	let signing = $state(false);
	let showInstanceSelect = $state(false);
	let showPublish = $state(false);
	let published = $state<RepoPublishResult | null>(null);
	let importingLabel = $state<string | null>(null);
	let installingLabel = $state<string | null>(null);
	let idTouched = $state(false);
	let kindTouched = $state(false);
	const queryClient = useQueryClient();

	let totalFiles = $derived(converter.sources.reduce((sum, s) => sum + s.files.length, 0));
	let idValid = $derived(MOD_ID_PATTERN.test(converter.modId.trim()));
	let idError = $derived(converter.modId.trim() !== "" && !idValid);
	let canCook = $derived(!!converter.modName.trim() && totalFiles > 0 && idValid);
	const repoHost = $derived(
		modRepositoryURL.value.replace(/^https?:\/\//, "").replace(/\/$/, ""),
	);
	const publishingEnabled = $derived(isFlagEnabled("mod-publishing"));
	const fmtSize = (n: number) =>
		n < 1024 ** 2 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1024 ** 2).toFixed(1)} MB`;

	// Derive the repository id from the name until the user edits it directly.
	$effect(() => {
		if (idTouched) return;
		converter.modId = slugify(converter.modName);
	});

	$effect(() => {
		if (converterPendingPaths.length > 0) {
			const paths = [...converterPendingPaths];
			converterPendingPaths.length = 0;
			// Read dropped files host-side in one batch, then process sequentially:
			// concurrent updates can overwrite each other and lose files when
			// several are dropped at once.
			void (async () => {
				const imports = await invoke("converter:read-imports", { paths });
				const skipped = paths.length - imports.length;
				if (skipped > 0) {
					addToast({
						title: m.toast_unsupported_file_title(),
						message: m.toast_unsupported_file_message(),
						tone: "error",
					});
				}
				for (const imported of imports) {
					await processFile(imported.path, new Uint8Array(imported.data));
				}
			})();
		}
	});

	async function pickFiles() {
		// Dialog + read both happen on the host in one round trip.
		const imports = await invoke("converter:pick-imports");
		for (const imported of imports) {
			await processFile(imported.path, new Uint8Array(imported.data));
		}
	}

	async function pickIcon() {
		const imported = await invoke("converter:pick-icon");
		if (!imported) return;
		converter.icon = { name: imported.name, data: new Uint8Array(imported.data) };
	}

	async function processFile(filePath: string, data: Uint8Array) {
		importingLabel = filePath.split(/[/\\]/).pop() ?? filePath;
		const ext = filePath.split(".").pop()?.toLowerCase();
		try {
			if (ext === "tempest") {
				await processTempest(filePath, data);
			} else if (ext === "upk" || ext === "pck" || ext === "ini") {
				const effective =
					ext === "upk" || ext === "pck" ? ((await optimizeUpk(filePath)) ?? data) : data;
				await processLooseFile(filePath, effective);
			} else {
				addToast({
					title: m.toast_unsupported_file_title(),
					message: m.toast_unsupported_file_message(),
					tone: "error",
				});
			}
		} catch (error) {
			console.error("Failed to import file:", filePath, error);
		} finally {
			if (!kindTouched) converter.kind = detectKind(allFiles());
			importingLabel = null;
		}
	}

	/**
	 * Compresses an uncompressed .upk/.pck with the bundled upk-parser so it
	 * installs like a game-cooked asset. Returns null when the file is already
	 * compressed or the parser cannot handle it — the original is used as-is.
	 * @param filePath Original path of the imported file (the parser reads it).
	 * @returns Compressed bytes, or null to keep the original.
	 */
	async function optimizeUpk(filePath: string): Promise<Uint8Array | null> {
		const name = filePath.split(/[/\\]/).pop() ?? filePath;
		importingLabel = m.converter_upk_optimizing({ name });
		try {
			const result = await invoke("upk:optimize", { path: filePath });
			if (result.optimized && result.data) {
				addToast({
					title: name,
					message: m.converter_upk_optimized({ name }),
					tone: "success",
				});
				return new Uint8Array(result.data);
			}
			return null;
		} catch (error) {
			// Unknown package layout: keep the file untouched rather than fail.
			console.warn("Could not optimize", filePath, error);
			return null;
		} finally {
			importingLabel = name;
		}
	}

	async function processTempest(filePath: string, data: Uint8Array) {
		try {
			const parsed = await parseV1Package(data);
			// UPKs embedded in the uploaded mod get the same treatment as loose
			// imports: compressed if they are not already.
			const compressedCount = await optimizeUpkEntries(parsed.files, (name) => {
				importingLabel = m.converter_upk_optimizing({ name });
			});
			if (compressedCount > 0) {
				const label0 = filePath.split(/[/\\]/).pop() ?? filePath;
				addToast({
					title: label0,
					message: m.converter_upk_optimized({ name: label0 }),
					tone: "success",
				});
			}
			const label = filePath.split(/[/\\]/).pop() ?? filePath;
			converter.sources = [
				...converter.sources,
				{ label, files: parsed.files, readme: parsed.readme ?? undefined },
			];

			const manifest = parsed.manifest;
			if (manifest && !converter.modName) {
				converter.modName = manifest.name || "";
				converter.modVersion = manifest.version || "1.0.0";
				if (manifest.authors?.length) {
					converter.authors = manifest.authors.map((a) => ({
						name: a.name,
						link: a.link || "",
					}));
				}
			}
			// A V1 manifest's id is often just the display name; only keep it when
			// it already satisfies the repository's slug rule.
			if (manifest?.id && MOD_ID_PATTERN.test(manifest.id)) {
				converter.modId = manifest.id;
				idTouched = true;
			}
			if (parsed.readme && !converter.readmeContent) {
				converter.readmeContent = parsed.readme;
			}
		} catch (error) {
			console.error("Failed to parse V1 mod:", error);
		}
	}

	function processLooseFile(filePath: string, data: Uint8Array) {
		const fileName = filePath.split(/[/\\]/).pop() ?? filePath;
		converter.sources = [
			...converter.sources,
			{
				label: fileName,
				files: [{ path: `${targetDirFor(fileName)}/${fileName}`, data }],
			},
		];
	}

	function allFiles(): { path: string; data: Uint8Array }[] {
		return converter.sources.flatMap((s) => s.files);
	}

	function updateTargetPath(sourceIdx: number, fileIdx: number, newPath: string) {
		const updated = [...converter.sources];
		updated[sourceIdx] = {
			...updated[sourceIdx],
			files: updated[sourceIdx].files.map((f, i) =>
				i === fileIdx ? { ...f, path: newPath } : f,
			),
		};
		converter.sources = updated;
	}

	function removeSource(index: number) {
		converter.sources = converter.sources.filter((_, i) => i !== index);
		if (!kindTouched) converter.kind = detectKind(allFiles());
	}

	function addAuthor() {
		converter.authors = [...converter.authors, { name: "", link: "" }];
	}

	function removeAuthor(index: number) {
		converter.authors = converter.authors.filter((_, i) => i !== index);
	}

	function updateAuthor(index: number, field: "name" | "link", value: string) {
		const updated = [...converter.authors];
		updated[index] = { ...updated[index], [field]: value };
		converter.authors = updated;
	}

	async function cook() {
		if (!canCook || cooking) return;
		cooking = true;
		signing = false;
		published = null;
		const previousStaged = converter.cooked?.stagedPath;
		converter.cooked = null;

		try {
			const id = converter.modId.trim();
			const { bytes, duplicates } = await buildPackageZip(
				{
					id,
					name: converter.modName.trim(),
					version: converter.modVersion.trim() || "1.0.0",
					authors: converter.authors,
					readme: converter.readmeContent,
					icon: converter.icon,
				},
				allFiles(),
			);
			if (duplicates > 0) {
				addToast({
					title: m.converter_source_files(),
					message: m.converter_duplicates_skipped(),
					tone: "warning",
				});
			}

			const filename = `${id}-${converter.modVersion.trim() || "1.0.0"}.tempest`;
			const stagedPath = await invoke("mods:stage-pack", { filename, data: bytes });

			// Sign host-side with the CLI (CHECKSUMS + CHECKSUMS.asc + SIGNER.pub);
			// a first-time publisher is asked to name their signing key.
			signing = true;
			const signer = await signWithPrompt(stagedPath);
			converter.cooked = { stagedPath, signer: signer || null, size: bytes.length };

			if (previousStaged && previousStaged !== stagedPath) {
				void invoke("mods:discard-staged-pack", { stagedPath: previousStaged }).catch(
					() => {},
				);
			}
		} catch (error) {
			console.error("Failed to cook mod:", error);
			addToast({
				title: m.converter_cook_failed(),
				message: error instanceof Error ? error.message : m.common_unknown(),
				tone: "error",
			});
		} finally {
			cooking = false;
			signing = false;
		}
	}

	async function download() {
		if (!converter.cooked) return;
		try {
			// Save dialog + copy of the signed package happen host-side.
			const savedPath = await invoke("mods:save-staged-pack", {
				stagedPath: converter.cooked.stagedPath,
			});
			if (!savedPath) return;
			addToast({
				title: m.converter_toast_saved_title(),
				message: m.converter_toast_saved_message({ path: savedPath }),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to save mod:", error);
		}
	}

	async function installToInstance(inst: Instance) {
		if (!converter.cooked || !inst?.path) return;
		const modFileName = `${converter.modId.trim() || converter.modName.trim() || "cooked-mod"}.tempest`;
		installingLabel = modFileName;
		void goto(`/instance/${inst.id}`);

		let installingToastId: string | undefined;
		try {
			installingToastId = addToast({
				title: m.toast_mod_installing_title(),
				message: m.toast_mod_installing_message({ name: modFileName }),
				tone: "info",
				duration: 0,
			});

			// The signed package is already a host-side temp file; install from there.
			const result = await installMod(inst.path, converter.cooked.stagedPath, true);

			if (installingToastId) removeToast(installingToastId);

			if (result.Success) {
				addToast({
					title: m.toast_mod_installed_title(),
					message: m.converter_toast_installed_message({
						name: converter.modName,
						instance: inst.label,
					}),
					tone: "success",
				});
				queryClient.invalidateQueries({ queryKey: ["mods", inst.path] });
			} else {
				addToast({
					title: m.toast_installation_failed_title(),
					message: result.Message || m.common_unknown(),
					tone: "error",
				});
			}
		} catch (error) {
			if (installingToastId) removeToast(installingToastId);
			console.error("Failed to install mod:", error);
		} finally {
			installingLabel = null;
		}
	}

	function addToInstance() {
		if (!converter.cooked) return;
		const instanceId = window.location.pathname.match(/^\/instance\/([^/]+)/)?.[1];
		if (instanceId && instanceMap.value[instanceId]?.path) {
			void installToInstance(instanceMap.value[instanceId]);
		} else {
			showInstanceSelect = true;
		}
	}

	function openPublish() {
		showPublish = true;
	}
</script>

<div class="h-full overflow-y-auto">
	<div class="mx-auto w-full max-w-6xl px-4 py-6">
		<div class="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_380px]">
			<!-- Sources -->
			<section class="card bg-base-200">
				<div class="card-body gap-4 p-4">
					<div class="flex items-center justify-between">
						<h2 class="text-sm font-semibold tracking-wide uppercase">
							{m.converter_source_files()}
						</h2>
						<button class="btn btn btn-sm gap-2" onclick={pickFiles}>
							<Plus size={16} />
							{m.common_browse()}
						</button>
					</div>

					{#if converter.sources.length === 0}
						<button
							class="border-base-300 rounded-box hover:border-primary/50 border-2 border-dashed p-10 text-center transition-colors"
							onclick={pickFiles}
						>
							<div class="text-base-content/50 flex flex-col items-center gap-2">
								<FileUp size={32} />
								<p class="text-sm">
									{m.converter_drop_hint({
										tempest: ".tempest",
										upk: ".upk",
										pck: ".pck",
										ini: ".ini",
									})}
								</p>
								<p class="text-xs opacity-60">{m.converter_drop_browse_hint()}</p>
							</div>
						</button>
					{:else}
						<div class="space-y-2">
							{#each converter.sources as source, sIdx (sIdx)}
								<div class="bg-base-300/50 rounded-lg p-3">
									<div class="mb-2 flex items-center justify-between gap-2">
										<div class="flex min-w-0 items-center gap-2">
											<span class="truncate font-mono text-sm font-semibold">
												{source.label}
											</span>
											<span class="badge badge-ghost badge-sm shrink-0">
												{source.files.length}
											</span>
										</div>
										<button
											class="btn btn-ghost btn-sm btn-square text-error"
											onclick={() => removeSource(sIdx)}
										>
											<Trash2 size={14} />
										</button>
									</div>
									<div class="space-y-1">
										{#each source.files as file, fIdx}
											<input
												type="text"
												class="input input-ghost input-sm w-full min-w-0 font-mono"
												value={file.path}
												oninput={(e) =>
													updateTargetPath(
														sIdx,
														fIdx,
														e.currentTarget.value,
													)}
											/>
										{/each}
									</div>
								</div>
							{/each}
						</div>
					{/if}

					{#if importingLabel}
						<div class="text-base-content/50 flex items-center gap-2 text-xs">
							<span class="loading loading-spinner loading-xs"></span>
							{m.converter_importing({ name: importingLabel })}
						</div>
					{:else if totalFiles > 0}
						<p class="text-base-content/50 text-xs">
							{totalFiles}
							{m.downloads_files({ count: totalFiles })} across {converter.sources
								.length}
							{m.converter_sources({ count: converter.sources.length })}
						</p>
					{/if}
				</div>
			</section>

			<!-- Details -->
			<section class="card bg-base-200">
				<div class="card-body gap-4 p-4">
					<h2 class="text-sm font-semibold tracking-wide uppercase">
						{m.converter_details_section()}
					</h2>

					<label class="form-control gap-1">
						<span class="text-sm font-semibold">{m.converter_mod_name_label()}</span>
						<input
							id="conv-name"
							type="text"
							class="input input-bordered"
							bind:value={converter.modName}
							placeholder={m.converter_mod_name_placeholder()}
						/>
					</label>

					<div class="grid grid-cols-[1fr_7rem] gap-2">
						<label class="form-control gap-1">
							<span class="text-sm font-semibold">{m.converter_mod_id_label()}</span>
							<input
								id="conv-id"
								type="text"
								class="input input-bordered font-mono"
								class:input-error={idError}
								bind:value={converter.modId}
								oninput={() => (idTouched = true)}
								placeholder={m.converter_mod_id_placeholder()}
								spellcheck="false"
							/>
						</label>
						<label class="form-control gap-1">
							<span class="text-sm font-semibold">{m.common_version()}</span>
							<input
								id="conv-version"
								type="text"
								class="input input-bordered"
								bind:value={converter.modVersion}
								placeholder={m.converter_version_placeholder()}
							/>
						</label>
					</div>
					<p class="text-base-content/50 -mt-2 text-xs">
						{idError ? m.converter_mod_id_invalid() : m.converter_mod_id_hint()}
					</p>

					<label class="form-control gap-1">
						<span class="text-sm font-semibold">{m.converter_summary_label()}</span>
						<input
							id="conv-summary"
							type="text"
							class="input input-bordered"
							bind:value={converter.summary}
							placeholder={m.converter_summary_placeholder()}
						/>
					</label>

					<div class="grid grid-cols-2 gap-2">
						<label class="form-control gap-1">
							<span class="text-sm font-semibold">{m.converter_kind_label()}</span>
							<select
								id="conv-kind"
								class="select select-bordered"
								bind:value={converter.kind}
								onchange={() => (kindTouched = true)}
							>
								<option value="assets">{m.repo_kind_assets()}</option>
								<option value="scripts">{m.repo_kind_scripts()}</option>
								<option value="native">{m.repo_kind_native()}</option>
							</select>
						</label>
						<label class="form-control gap-1">
							<span class="text-sm font-semibold">{m.converter_tags_label()}</span>
							<input
								id="conv-tags"
								type="text"
								class="input input-bordered"
								bind:value={converter.tags}
								placeholder={m.converter_tags_placeholder()}
							/>
						</label>
					</div>

					<div class="form-control gap-1">
						<span class="text-sm font-semibold">{m.converter_icon_label()}</span>
						{#if converter.icon}
							<div
								class="bg-base-300/50 flex items-center justify-between rounded-lg px-3 py-2"
							>
								<span class="truncate font-mono text-xs">{converter.icon.name}</span
								>
								<button
									class="btn btn-ghost btn-xs btn-square text-error"
									onclick={() => (converter.icon = null)}
								>
									<X size={14} />
								</button>
							</div>
						{:else}
							<button
								class="border-base-300 flex items-center gap-2 rounded-lg border-2 border-dashed px-3 py-2 text-sm opacity-70 hover:opacity-100"
								onclick={pickIcon}
							>
								<ImageIcon size={16} />
								{m.converter_icon_pick()}
							</button>
						{/if}
					</div>

					<div class="form-control gap-1">
						<div class="flex items-center justify-between">
							<span class="text-sm font-semibold">{m.converter_authors_label()}</span>
							<button class="btn btn-ghost btn-xs gap-1" onclick={addAuthor}>
								<Plus size={14} />
								{m.common_add()}
							</button>
						</div>
						<div class="space-y-2">
							{#each converter.authors as author, i}
								<div class="bg-base-300/50 rounded-lg p-2">
									<div class="flex flex-col gap-1">
										<input
											type="text"
											class="input input-bordered input-sm"
											placeholder={m.converter_author_name_placeholder()}
											value={author.name}
											oninput={(e) =>
												updateAuthor(i, "name", e.currentTarget.value)}
										/>
										<div class="flex items-center gap-1">
											<input
												type="text"
												class="input input-bordered input-sm flex-1"
												placeholder={m.converter_author_link_placeholder()}
												value={author.link}
												oninput={(e) =>
													updateAuthor(i, "link", e.currentTarget.value)}
											/>
											<button
												class="btn btn-ghost btn-sm btn-square text-error shrink-0"
												onclick={() => removeAuthor(i)}
											>
												<X size={14} />
											</button>
										</div>
									</div>
								</div>
							{/each}
						</div>
					</div>
				</div>
			</section>
		</div>

		<!-- Readme -->
		<section class="card bg-base-200 mt-4">
			<div class="card-body gap-2 p-4">
				<h2 class="text-sm font-semibold tracking-wide uppercase">
					{m.converter_readme_label()}
				</h2>
				<textarea
					id="conv-readme"
					class="textarea textarea-bordered h-40 w-full"
					bind:value={converter.readmeContent}
					placeholder={m.converter_readme_placeholder()}></textarea>
			</div>
		</section>

		<!-- Cooked package -->
		{#if converter.cooked}
			<section class="card bg-base-200 mt-4">
				<div class="card-body gap-3 p-4">
					<div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
						<h2 class="text-lg font-bold">{converter.modName || converter.modId}</h2>
						<span class="badge badge-primary badge-sm"
							>v{converter.modVersion || "1.0.0"}</span
						>
						<span class="text-base-content/40 font-mono text-xs"
							>{converter.modId}.tempest</span
						>
					</div>
					<div class="flex flex-wrap items-center gap-2 text-xs">
						<span class="badge badge-success badge-sm gap-1">
							<BadgeCheck size={12} />
							{m.converter_package_ready()}
						</span>
						{#if converter.cooked.signer}
							<span class="max-w-72 truncate font-mono opacity-60">
								{m.converter_signed_by({ signer: converter.cooked.signer })}
							</span>
						{/if}
						<span class="opacity-60">{fmtSize(converter.cooked.size)}</span>
					</div>
					<div class="flex flex-wrap items-center gap-2">
						{#if publishingEnabled}
							<button class="btn btn-primary gap-2" onclick={openPublish}>
								<Upload size={16} />
								{m.converter_publish_btn()}
							</button>
						{/if}
						<button class="btn gap-2" onclick={download}>
							<FolderOpen size={16} />
							{m.converter_download()}
						</button>
						<button
							class="btn gap-2"
							disabled={installingLabel !== null}
							onclick={addToInstance}
						>
							<File size={16} />
							{m.converter_add_to_instance()}
						</button>
					</div>
					{#if publishingEnabled && published?.state === "done" && published.modId}
						<div class="bg-success/10 rounded-lg px-3 py-2">
							<div class="text-success flex items-center gap-2 text-sm font-medium">
								<CheckCircle2 size={16} class="shrink-0" />
								{published.version
									? m.converter_publish_done({
											id: published.modId,
											version: published.version,
										})
									: m.converter_toast_published_title()}
							</div>
						</div>
						<div>
							<button
								class="link text-xs hover:underline"
								onclick={() =>
									openUrl(
										`${modRepositoryURL.value}/mods/${published?.modId ?? ""}`,
									)}
							>
								{m.converter_view_online()}
							</button>
						</div>
					{:else}
						<p class="text-base-content/50 text-xs">
							{m.converter_publish_manual()}
							<button
								class="link hover:underline"
								onclick={() => openUrl(`${repoHost}/`)}
							>
								{m.converter_publish_manual_link()}
							</button>
						</p>
					{/if}
				</div>
			</section>
		{/if}

		<!-- Cook action -->
		<div class="from-base-100 sticky bottom-0 mt-4 bg-gradient-to-t from-60% pt-4 pb-2">
			<button
				class="btn btn-primary btn-lg gap-2"
				disabled={!canCook || cooking}
				onclick={cook}
			>
				{#if cooking}
					<span class="loading loading-spinner loading-xs"></span>
					{#if signing}
						{m.converter_signing()}
					{:else}
						{m.converter_cooking()}
					{/if}
				{:else}
					<FlaskConical size={20} />
					{m.converter_cook()}
				{/if}
			</button>
		</div>
	</div>
</div>

<InstanceSelectModal
	bind:open={showInstanceSelect}
	onselect={(inst) => {
		void installToInstance(inst);
	}}
	oncancel={() => {
		showInstanceSelect = false;
	}}
/>

<PublishModal
	bind:open={showPublish}
	onpublished={(result) => {
		published = result;
		if (result.state === "done" && result.modId && result.version) {
			addToast({
				title: m.converter_toast_published_title(),
				message: m.converter_toast_published_message({
					id: result.modId,
					version: result.version,
				}),
				tone: "success",
			});
		}
	}}
/>
