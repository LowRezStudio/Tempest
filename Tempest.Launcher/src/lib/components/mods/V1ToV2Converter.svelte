<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		CheckCircle2,
		Circle,
		CircleCheck,
		File,
		FileArchive,
		FileUp,
		FolderOpen,
		ImagePlus,
		Lock,
		Plus,
		RotateCcw,
		TriangleAlert,
		Upload,
		X,
		XCircle,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { tick, type Snippet } from "svelte";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import PublishModal from "$lib/components/mods/PublishModal.svelte";
	import FieldError from "$lib/components/ui/FieldError.svelte";
	import { installMod, type RepoPublishResult } from "$lib/core/mods";
	import { converterPendingPaths, isDraggingConverterFiles } from "$lib/mods/drop.svelte";
	import {
		MOD_ID_PATTERN,
		buildPackageZip,
		optimizeUpkEntries,
		parsePackage,
		looseFileEntry,
		slugify,
		type FileEntry,
	} from "$lib/mods/packaging";
	import { signWithPrompt } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openUrl } from "$lib/platform/api";
	import { converter } from "$lib/stores/converter.svelte";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { modRepositoryURL } from "$lib/stores/settings.svelte";
	import { addToast, removeToast } from "$lib/stores/ui.svelte";
	import { isSemver, toSemver } from "$lib/utils/deps";
	import type { Instance } from "$lib/types/instance";

	/**
	 * Cooking as four steps that open one after another, like publishing on the mod website: the
	 * files first (a .tempest fills in the rest), then the details, the listing, and icon and
	 * readme last. Earlier steps stay editable; a sticky column beside them tracks progress and
	 * holds Cook and the cooked package.
	 */

	const MAX_SUMMARY = 140;

	let cooking = $state(false);
	let signing = $state(false);
	let cookError = $state<string | null>(null);
	let confirmReset = $state(false);
	let showInstanceSelect = $state(false);
	let showPublish = $state(false);
	let published = $state<RepoPublishResult | null>(null);
	let importingLabel = $state<string | null>(null);
	let installingLabel = $state<string | null>(null);
	const queryClient = useQueryClient();

	let totalFiles = $derived(converter.sources.reduce((sum, s) => sum + s.files.length, 0));
	let idValid = $derived(MOD_ID_PATTERN.test(converter.modId.trim()));
	let idError = $derived(converter.modId.trim() !== "" && !idValid);
	// Left empty, the version falls back to the placeholder's 1.0.0.
	let versionValid = $derived(
		converter.modVersion.trim() === "" || isSemver(converter.modVersion.trim()),
	);
	let versionSuggestion = $derived(versionValid ? null : toSemver(converter.modVersion));
	const repoHost = $derived(
		modRepositoryURL.value.replace(/^https?:\/\//, "").replace(/\/$/, ""),
	);
	const publishingEnabled = $derived(isFlagEnabled("mod-publishing"));
	const fmtSize = (n: number) =>
		n < 1024 ** 2 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1024 ** 2).toFixed(1)} MB`;

	// invalid: holds a value that won't be accepted, as opposed to one not filled in yet.
	const steps = $derived([
		{
			id: "files",
			title: m.converter_step_files(),
			valid: totalFiles > 0,
			required: true,
			invalid: false,
		},
		{
			id: "details",
			title: m.converter_step_details(),
			valid: !!converter.modName.trim() && idValid && versionValid,
			required: true,
			invalid: idError || !versionValid,
		},
		{
			id: "listing",
			title: m.converter_step_listing(),
			valid: !!converter.summary.trim(),
			required: false,
			invalid: false,
		},
		{
			id: "extras",
			title: m.converter_step_extras(),
			valid: !!converter.icon || !!converter.readmeContent.trim(),
			required: false,
			invalid: false,
		},
	]);
	// done: moved past (filled in, or optional); current: the step to work on; locked: not opened yet.
	const statusOf = (i: number) =>
		i > converter.reached
			? "locked"
			: i < converter.reached && (steps[i].valid || !steps[i].required)
				? "done"
				: "current";
	const ready = $derived(
		converter.reached === steps.length - 1 && steps.every((s) => !s.required || s.valid),
	);
	/** What to do before Cook works, for its tooltip */
	const blocker = $derived(
		converter.reached < steps.length - 1
			? steps[converter.reached].title
			: steps.find((s) => s.required && !s.valid)?.title,
	);

	const iconUrl = $derived(
		converter.icon
			? URL.createObjectURL(new Blob([new Uint8Array(converter.icon.data)]))
			: null,
	);
	$effect(() => {
		const url = iconUrl;
		return () => {
			if (url) URL.revokeObjectURL(url);
		};
	});

	// What goes into the package; a package cooked from anything else is out of date.
	const inputKey = $derived(
		JSON.stringify({
			id: converter.modId.trim(),
			name: converter.modName.trim(),
			version: converter.modVersion.trim(),
			authors: converter.authors,
			readme: converter.readmeContent,
			dependencies: converter.dependencyList,
			icon: converter.icon && [converter.icon.name, converter.icon.data.length],
			files: allFiles().map((f) => [f.path, f.data.length, !!f.inModFolder]),
		}),
	);
	const stale = $derived(!!converter.cooked && converter.cooked.from !== inputKey);

	// The repository id follows the package's, else the name, until the user edits it directly.
	$effect(() => {
		if (converter.idTouched) return;
		converter.modId = converter.packageId ?? slugify(converter.modName);
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

	function scrollToStep(i: number) {
		document
			.querySelector(`#step-${steps[i].id}`)
			?.scrollIntoView({ behavior: "smooth", block: "start" });
	}

	async function advance(i: number) {
		converter.reached = Math.max(converter.reached, i + 1);
		await tick();
		scrollToStep(i + 1);
	}

	// Ctrl+Enter: Continue on the open step, or Cook on the last one.
	function onKeydown(e: KeyboardEvent) {
		if (e.key !== "Enter" || !(e.ctrlKey || e.metaKey) || cooking) return;
		if (showPublish || showInstanceSelect) return;
		if (converter.reached < steps.length - 1) {
			const open = steps[converter.reached];
			if (converter.reached === 0 || (open.required && !open.valid)) return;
			e.preventDefault();
			// The field typed in belongs to the step just left; it shouldn't stay focused.
			if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
			void advance(converter.reached);
		} else if (ready) {
			e.preventDefault();
			void cook();
		}
	}

	/** Empties the cooker; asks once more first. */
	function startOver() {
		if (!confirmReset) {
			confirmReset = true;
			setTimeout(() => (confirmReset = false), 3000);
			return;
		}
		confirmReset = false;
		clear();
		scrollToStep(0);
	}

	function clear() {
		const staged = converter.cooked?.stagedPath;
		if (staged) void invoke("mods:discard-staged-pack", { stagedPath: staged }).catch(() => {});
		converter.reset();
		cookError = null;
		published = null;
	}

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
		const name = filePath.split(/[/\\]/).pop() ?? filePath;
		importingLabel = name;
		const ext = filePath.split(".").pop()?.toLowerCase();
		try {
			if (ext === "tempest") {
				await processTempest(name, data);
			} else if (ext === "upk" || ext === "pck") {
				processLooseFile(filePath, (await optimizeUpk(filePath)) ?? data);
			} else if (ext === "ini" || ext === "dll" || ext === "lua") {
				processLooseFile(filePath, data);
			} else {
				addToast({
					title: m.toast_unsupported_file_title(),
					message: m.toast_unsupported_file_message(),
					tone: "error",
				});
			}
		} catch (error) {
			console.error("Failed to import file:", filePath, error);
			addToast({
				title: m.converter_import_failed({ name }),
				message: error instanceof Error ? error.message : m.common_unknown(),
				tone: "error",
			});
		} finally {
			importingLabel = null;
			if (converter.reached === 0 && totalFiles > 0) void advance(0);
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

	async function processTempest(label: string, data: Uint8Array) {
		const parsed = await parsePackage(data);
		if (parsed.files.length === 0) throw new Error(m.converter_no_files());
		// UPKs embedded in the package get the same treatment as loose imports:
		// compressed if they are not already.
		const compressedCount = await optimizeUpkEntries(parsed.files, (name) => {
			importingLabel = m.converter_upk_optimizing({ name });
		});
		if (compressedCount > 0) {
			addToast({
				title: label,
				message: m.converter_upk_optimized({ name: label }),
				tone: "success",
			});
		}
		const manifest = parsed.manifest;
		converter.sources = [
			...converter.sources,
			{
				label,
				files: parsed.files,
				meta: {
					name: manifest?.name ?? "",
					id: manifest?.id ?? "",
					version: manifest?.version ?? "",
					authors: manifest?.authors ?? [],
					readme: parsed.readme ?? "",
					icon: parsed.icon,
					dependencies: manifest?.dependencies ?? [],
				},
			},
		];
		converter.syncFromSources();
	}

	function processLooseFile(filePath: string, data: Uint8Array) {
		const fileName = filePath.split(/[/\\]/).pop() ?? filePath;
		converter.sources = [
			...converter.sources,
			{ label: fileName, files: [looseFileEntry(fileName, data)] },
		];
		converter.syncFromSources();
	}

	function allFiles(): FileEntry[] {
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

	// Everything else is about the files, so without any the cooker starts over.
	function removeSource(index: number) {
		if (converter.sources.length === 1) {
			clear();
			scrollToStep(0);
			return;
		}
		converter.sources = converter.sources.filter((_, i) => i !== index);
		converter.syncFromSources();
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
		if (!ready || cooking) return;
		cooking = true;
		signing = false;
		cookError = null;
		published = null;
		const from = inputKey;
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
					dependencies: converter.dependencyList,
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
			converter.cooked = { stagedPath, signer: signer || null, size: bytes.length, from };

			if (previousStaged && previousStaged !== stagedPath) {
				void invoke("mods:discard-staged-pack", { stagedPath: previousStaged }).catch(
					() => {},
				);
			}
		} catch (error) {
			console.error("Failed to cook mod:", error);
			cookError = error instanceof Error ? error.message : m.common_unknown();
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
</script>

<!-- One step of the form: numbered, locked until the step before it is done -->
{#snippet panel(i: number, body: Snippet, aside: Snippet | null = null)}
	{@const st = steps[i]}
	{@const status = statusOf(i)}
	<section
		id={`step-${st.id}`}
		class={[
			"bg-base-200 rounded-box @container scroll-mt-6 border p-4 transition-[opacity,border-color]",
			st.invalid
				? "border-error/70"
				: status === "current"
					? "border-primary/60"
					: "border-base-300",
			status === "locked" && "opacity-50",
		]}
		inert={status === "locked"}
		aria-current={status === "current" ? "step" : undefined}
	>
		<div class="mb-3 flex items-center gap-2">
			<span
				class={[
					"flex size-5 shrink-0 items-center justify-center rounded-full text-xs font-semibold",
					st.invalid
						? "bg-error text-error-content"
						: [
								status === "done" && "bg-success text-success-content",
								status === "current" && "bg-primary text-primary-content",
								status === "locked" && "bg-base-300",
							],
				]}>{i + 1}</span
			>
			<h2 class="text-sm font-semibold">
				{st.title}
				{#if !st.required}<span class="text-base-content/50 font-normal"
						>· {m.converter_step_optional()}</span
					>{/if}
			</h2>
			{#if status === "locked"}
				<span class="text-base-content/60 ml-auto flex items-center gap-1 text-xs"
					><Lock size={12} />
					{m.converter_step_locked({ step: steps[i - 1].title.toLowerCase() })}</span
				>
			{:else if aside}<div class="ml-auto">{@render aside()}</div>{/if}
		</div>
		{@render body()}
		{#if i === converter.reached && i > 0 && i < steps.length - 1}
			<div class="mt-4 flex items-center justify-end gap-3">
				<span class="text-base-content/50 text-xs">{m.converter_continue_hint()}</span>
				<button
					type="button"
					class="btn btn-primary btn-sm"
					disabled={!st.valid && st.required}
					onclick={() => advance(i)}>{m.converter_continue()}</button
				>
			</div>
		{/if}
	</section>
{/snippet}

{#snippet filesAside()}
	<button type="button" class="btn btn-ghost btn-xs" onclick={pickFiles}>
		<Plus size={12} />
		{m.common_browse()}
	</button>
{/snippet}

{#snippet filesBody()}
	{#if converter.sources.length === 0}
		<button
			type="button"
			class={[
				"rounded-field flex w-full cursor-pointer flex-col items-center gap-1 border-2 border-dashed px-4 py-8 text-center transition-colors",
				isDraggingConverterFiles.value
					? "border-primary bg-primary/5"
					: "border-base-300 hover:border-primary/60",
			]}
			onclick={pickFiles}
		>
			<FileUp size={28} class="mb-1 opacity-60" />
			<span class="font-medium">
				{m.converter_drop_hint({
					tempest: ".tempest",
					upk: ".upk",
					pck: ".pck",
					ini: ".ini",
					dll: ".dll",
					lua: ".lua",
				})}
			</span>
			<span class="text-base-content/60 text-sm">{m.converter_drop_browse_hint()}</span>
		</button>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each converter.sources as source, sIdx (sIdx)}
				{@const Icon = source.label.toLowerCase().endsWith(".tempest") ? FileArchive : File}
				<li class="bg-base-100 border-base-300 rounded-field border p-3">
					<div class="flex items-center gap-3">
						<Icon size={20} class="text-primary shrink-0" />
						<div class="min-w-0 flex-1">
							<p class="truncate font-mono text-sm">{source.label}</p>
							<p class="text-base-content/60 text-xs">
								{source.files.length}
								{m.downloads_files({ count: source.files.length })} · {fmtSize(
									source.files.reduce((n, f) => n + f.data.length, 0),
								)}
							</p>
						</div>
						<button
							type="button"
							class="btn btn-ghost btn-sm btn-square"
							aria-label={m.common_remove()}
							onclick={() => removeSource(sIdx)}
						>
							<X size={14} />
						</button>
					</div>
					<div class="mt-2 flex flex-col gap-1">
						{#each source.files as file, fIdx}
							<input
								type="text"
								class="input input-xs w-full min-w-0 font-mono"
								value={file.path}
								spellcheck="false"
								oninput={(e) => updateTargetPath(sIdx, fIdx, e.currentTarget.value)}
							/>
						{/each}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
	<p class="text-base-content/60 mt-2 flex items-center gap-2 text-xs">
		{#if importingLabel}
			<span class="loading loading-spinner loading-xs"></span>
			{m.converter_importing({ name: importingLabel })}
		{:else if totalFiles > 0}
			{m.converter_files_count({
				files: `${totalFiles} ${m.downloads_files({ count: totalFiles })}`,
				sources: `${converter.sources.length} ${m.converter_sources({ count: converter.sources.length })}`,
			})}
		{:else}
			{m.converter_files_hint()}
		{/if}
	</p>
{/snippet}

{#snippet detailsBody()}
	<div class="grid gap-3 @lg:grid-cols-2">
		<label class="flex flex-col gap-1 @lg:col-span-2">
			<span class="text-base-content/60 text-xs">{m.converter_mod_name_label()}</span>
			<input
				class="input input-sm w-full"
				placeholder={m.converter_mod_name_placeholder()}
				bind:value={converter.modName}
			/>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.converter_mod_id_label()}</span>
			<input
				class="input input-sm w-full font-mono"
				placeholder={m.converter_mod_id_placeholder()}
				spellcheck="false"
				aria-invalid={idError}
				aria-describedby={idError ? "mod-id-error" : undefined}
				bind:value={converter.modId}
				oninput={() => (converter.idTouched = true)}
			/>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.common_version()}</span>
			<input
				class="input input-sm w-full font-mono"
				placeholder={m.converter_version_placeholder()}
				spellcheck="false"
				aria-invalid={!versionValid}
				aria-describedby={versionValid ? undefined : "mod-version-error"}
				bind:value={converter.modVersion}
			/>
		</label>
		{#if idError}
			<FieldError
				id="mod-id-error"
				class="@lg:col-span-2"
				title={m.converter_mod_id_invalid_title({ id: converter.modId.trim() })}
				message={m.converter_mod_id_invalid()}
			/>
		{:else}
			<p class="text-base-content/60 -mt-1 text-xs @lg:col-span-2">
				{m.converter_mod_id_hint()}
			</p>
		{/if}
		{#if !versionValid}
			<FieldError
				id="mod-version-error"
				class="@lg:col-span-2"
				title={m.converter_version_invalid_title({ version: converter.modVersion.trim() })}
				message={m.converter_version_invalid()}
				fix={versionSuggestion
					? {
							label: m.converter_version_suggestion({ version: versionSuggestion }),
							apply: () => (converter.modVersion = versionSuggestion ?? ""),
						}
					: undefined}
			/>
		{/if}
		<div class="flex flex-col gap-1 @lg:col-span-2">
			<span class="text-base-content/60 flex items-center justify-between text-xs">
				<span>{m.converter_authors_label()}</span>
				<button type="button" class="btn btn-ghost btn-xs" onclick={addAuthor}>
					<Plus size={12} />
					{m.common_add()}
				</button>
			</span>
			{#each converter.authors as author, i}
				<div class="flex items-center gap-2">
					<input
						class="input input-sm min-w-0 flex-1"
						placeholder={m.converter_author_name_placeholder()}
						value={author.name}
						oninput={(e) => updateAuthor(i, "name", e.currentTarget.value)}
					/>
					<input
						class="input input-sm min-w-0 flex-1"
						placeholder={m.converter_author_link_placeholder()}
						value={author.link}
						oninput={(e) => updateAuthor(i, "link", e.currentTarget.value)}
					/>
					<button
						type="button"
						class="btn btn-ghost btn-sm btn-square shrink-0"
						aria-label={m.common_remove()}
						onclick={() => removeAuthor(i)}
					>
						<X size={14} />
					</button>
				</div>
			{/each}
		</div>
	</div>
{/snippet}

{#snippet listingBody()}
	<div class="grid gap-3 @lg:grid-cols-2">
		<label class="flex flex-col gap-1 @lg:col-span-2">
			<span class="text-base-content/60 flex justify-between text-xs"
				><span>{m.converter_summary_label()}</span><span
					>{converter.summary.length}/{MAX_SUMMARY}</span
				></span
			>
			<input
				class="input input-sm w-full"
				placeholder={m.converter_summary_placeholder()}
				maxlength={MAX_SUMMARY}
				bind:value={converter.summary}
			/>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.converter_kind_label()}</span>
			<select
				class="select select-sm w-full"
				bind:value={converter.kind}
				onchange={() => (converter.kindTouched = true)}
			>
				<option value="assets">{m.repo_kind_assets()}</option>
				<option value="scripts">{m.repo_kind_scripts()}</option>
				<option value="native">{m.repo_kind_native()}</option>
			</select>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.converter_tags_label()}</span>
			<input
				class="input input-sm w-full"
				placeholder={m.converter_tags_placeholder()}
				bind:value={converter.tags}
			/>
		</label>
		<label class="flex flex-col gap-1 @lg:col-span-2">
			<span class="text-base-content/60 text-xs">{m.converter_dependencies_label()}</span>
			<textarea
				class="textarea textarea-sm min-h-16 w-full font-mono"
				placeholder={m.converter_dependencies_placeholder()}
				bind:value={converter.dependencies}></textarea>
			<span class="text-base-content/60 text-xs">{m.converter_dependencies_hint()}</span>
		</label>
		<p class="text-base-content/60 text-xs @lg:col-span-2">{m.converter_listing_hint()}</p>
	</div>
{/snippet}

{#snippet extrasBody()}
	<div class="grid gap-3 @lg:grid-cols-[8rem_minmax(0,1fr)]">
		<div class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.converter_icon_label()}</span>
			{#if converter.icon && iconUrl}
				<div class="rounded-field bg-base-300 relative aspect-square w-32 overflow-hidden">
					<img
						src={iconUrl}
						alt={converter.icon.name}
						title={converter.icon.name}
						class="h-full w-full object-cover"
					/>
					<button
						type="button"
						class="btn btn-circle btn-xs absolute top-1 right-1"
						aria-label={m.converter_icon_clear()}
						onclick={() => (converter.icon = null)}><X size={12} /></button
					>
				</div>
			{:else}
				<button
					type="button"
					class="border-base-300 hover:border-primary/60 rounded-field text-base-content/60 flex aspect-square w-32 cursor-pointer flex-col items-center justify-center gap-1 border-2 border-dashed text-xs transition-colors"
					onclick={pickIcon}
				>
					<ImagePlus size={18} />
					<span>{m.converter_icon_pick()}</span>
				</button>
			{/if}
		</div>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.converter_readme_label()}</span>
			<textarea
				class="textarea textarea-sm min-h-32 w-full flex-1"
				placeholder={m.converter_readme_placeholder()}
				bind:value={converter.readmeContent}></textarea>
		</label>
	</div>
{/snippet}

{#snippet checklist()}
	<ol class="flex flex-col gap-0.5 text-sm">
		{#each steps as st, i (st.id)}
			{@const status = statusOf(i)}
			<li>
				<button
					type="button"
					class={[
						"rounded-field flex w-full items-center gap-2 px-2 py-1 text-left",
						status === "locked" ? "text-base-content/40" : "hover:bg-base-300",
						st.invalid
							? "bg-error/10 text-error font-medium"
							: status === "current" && "bg-primary/10 font-medium",
					]}
					disabled={status === "locked"}
					onclick={() => scrollToStep(i)}
				>
					{#if st.invalid}<XCircle size={14} />
					{:else if status === "done"}<CircleCheck size={14} class="text-success" />
					{:else if status === "locked"}<Lock size={14} />
					{:else}<Circle size={14} class="text-primary" />{/if}
					{st.title}
					{#if !st.required}<span class="text-base-content/40 ml-auto text-xs font-normal"
							>{m.converter_step_optional()}</span
						>{/if}
				</button>
			</li>
		{/each}
	</ol>
{/snippet}

<!-- What came of cooking: the signed package and what to do with it, or why it failed -->
{#snippet outcome()}
	{#if cookError}
		<div
			role="alert"
			class="border-error/40 bg-error/10 rounded-box border p-3 text-sm wrap-anywhere"
		>
			<p class="flex items-start gap-2 font-semibold">
				<XCircle size={16} class="text-error mt-0.5 shrink-0" />
				{m.converter_cook_failed()}
			</p>
			<p class="text-base-content/80 mt-2">{cookError}</p>
		</div>
	{/if}
	{#if converter.cooked && stale}
		<div class="border-warning/40 bg-warning/10 rounded-box border p-3 text-sm">
			<p class="flex items-start gap-2 font-semibold">
				<TriangleAlert size={16} class="text-warning mt-0.5 shrink-0" />
				{m.converter_cooked_stale_title()}
			</p>
			<p class="text-base-content/80 mt-2">{m.converter_cooked_stale_hint()}</p>
		</div>
	{:else if converter.cooked}
		<div
			class="border-success/40 bg-success/10 rounded-box flex flex-col gap-3 border p-3 text-sm wrap-anywhere"
		>
			<div>
				<p class="flex items-start gap-2 font-semibold">
					<CheckCircle2 size={16} class="text-success mt-0.5 shrink-0" />
					{m.converter_package_ready()}
				</p>
				<p class="mt-2">
					<span class="font-mono">{converter.modId}</span>
					v{converter.modVersion || "1.0.0"}
					<span class="text-base-content/60">· {fmtSize(converter.cooked.size)}</span>
				</p>
				{#if converter.cooked.signer}
					<p class="text-base-content/60 mt-0.5 font-mono text-xs">
						{m.converter_signed_by({ signer: converter.cooked.signer })}
					</p>
				{/if}
			</div>
			<div class="flex flex-col gap-2">
				{#if publishingEnabled}
					<button
						type="button"
						class="btn btn-primary btn-sm w-full"
						onclick={() => (showPublish = true)}
					>
						<Upload size={14} />
						{m.converter_publish_btn()}
					</button>
				{/if}
				<button type="button" class="btn btn-sm w-full" onclick={download}>
					<FolderOpen size={14} />
					{m.converter_download()}
				</button>
				<button
					type="button"
					class="btn btn-sm w-full"
					disabled={installingLabel !== null}
					onclick={addToInstance}
				>
					<File size={14} />
					{m.converter_add_to_instance()}
				</button>
			</div>
			{#if publishingEnabled && published?.state === "done" && published.modId}
				<p class="text-base-content/80">
					{published.version
						? m.converter_publish_done({
								id: published.modId,
								version: published.version,
							})
						: m.converter_toast_published_title()} ·
					<button
						type="button"
						class="link"
						onclick={() =>
							openUrl(`${modRepositoryURL.value}/mods/${published?.modId ?? ""}`)}
					>
						{m.converter_view_online()}
					</button>
				</p>
			{:else}
				<p class="text-base-content/70 text-xs">
					{m.converter_publish_manual()}
					<button type="button" class="link" onclick={() => openUrl(`${repoHost}/`)}>
						{m.converter_publish_manual_link()}
					</button>
				</p>
			{/if}
		</div>
	{/if}
{/snippet}

<svelte:window onkeydown={onKeydown} />

<div class="@container h-full overflow-y-auto">
	<div
		class="mx-auto grid w-full max-w-6xl items-start gap-4 px-4 py-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]"
	>
		<div class="flex min-w-0 flex-col gap-4">
			{@render panel(0, filesBody, filesAside)}
			{@render panel(1, detailsBody)}
			{@render panel(2, listingBody)}
			{@render panel(3, extrasBody)}
		</div>
		<aside class="flex flex-col gap-3 @3xl:sticky @3xl:top-6">
			<div class="bg-base-200 border-base-300 rounded-box flex flex-col gap-3 border p-4">
				<nav aria-label={m.converter_steps()}>{@render checklist()}</nav>
				<button
					type="button"
					class="btn btn-primary w-full"
					disabled={!ready || cooking}
					title={!ready && blocker
						? m.converter_cook_blocked({ step: blocker })
						: undefined}
					onclick={cook}
				>
					{#if cooking}<span class="loading loading-spinner loading-xs"></span>{/if}
					{cooking
						? signing
							? m.converter_signing()
							: m.converter_cooking()
						: m.converter_cook()}
				</button>
				{#if totalFiles > 0 || converter.reached > 0}
					<div class="flex justify-end">
						<button
							type="button"
							class={["btn btn-xs", confirmReset ? "btn-error" : "btn-ghost"]}
							disabled={cooking}
							onclick={startOver}
							><RotateCcw size={12} />
							{confirmReset
								? m.converter_start_over_confirm()
								: m.converter_start_over()}</button
						>
					</div>
				{/if}
			</div>
			{@render outcome()}
		</aside>
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
