<script lang="ts">
	import { goto } from "$app/navigation";
	import { File, FileUp, FlaskConical, FolderOpen, Plus, Trash2, X } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { path } from "@tauri-apps/api";
	import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
	import { readFile } from "@tauri-apps/plugin-fs";
	import JSZip from "jszip";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { installMod } from "$lib/core/mods";
	import { converterPendingPaths, isDraggingConverterFiles } from "$lib/mods/drop.svelte";
	import { m } from "$lib/paraglide/messages";
	import { converter } from "$lib/stores/converter.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { addToast, removeToast } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	type FileEntry = { path: string; data: Uint8Array };
	type SourceEntry = {
		label: string;
		files: FileEntry[];
		manifest?: {
			name: string;
			id: string;
			version: string;
			authors: { name: string; link: string }[];
		};
		readme?: string;
	};

	function targetDirFor(fileName: string): string {
		const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
		if (ext === "ini") return "ChaosGame/Config";
		const name = fileName.toUpperCase();
		const isVoice = name.includes("_VOX") || name.includes("_VGS");
		return isVoice ? "ChaosGame/CookedPCConsole/English(US)" : "ChaosGame/CookedPCConsole";
	}

	let cooking = $state(false);
	let showInstanceSelect = $state(false);
	let importingLabel = $state<string | null>(null);
	let installingLabel = $state<string | null>(null);
	const queryClient = useQueryClient();

	let totalFiles = $derived(converter.sources.reduce((sum, s) => sum + s.files.length, 0));
	let canCook = $derived(!!converter.modName.trim() && totalFiles > 0);

	$effect(() => {
		if (converterPendingPaths.length > 0) {
			const paths = [...converterPendingPaths];
			converterPendingPaths.length = 0;
			for (const p of paths) {
				void processFile(p);
			}
		}
	});

	async function pickFiles() {
		const result = await openDialog({
			multiple: true,
			filters: [{ name: "Mod Files", extensions: ["tempest", "upk", "pck", "ini"] }],
		});
		if (!result) return;
		for (const f of Array.isArray(result) ? result : [result]) {
			await processFile(f);
		}
	}

	async function processFile(filePath: string) {
		importingLabel = filePath.split(/[/\\]/).pop() ?? filePath;
		const ext = filePath.split(".").pop()?.toLowerCase();
		try {
			if (ext === "tempest") {
				await processTempest(filePath);
			} else if (ext === "upk" || ext === "pck" || ext === "ini") {
				await processLooseFile(filePath);
			} else {
				addToast({
					title: m.toast_unsupported_file_title(),
					message: m.toast_unsupported_file_message(),
					tone: "error",
				});
			}
		} finally {
			importingLabel = null;
		}
	}

	async function processTempest(filePath: string) {
		try {
			const data = await readFile(filePath);
			const zip = await JSZip.loadAsync(data);
			const files: FileEntry[] = [];
			let manifest: SourceEntry["manifest"] | undefined;
			let readme: string | undefined;

			const manifestFile = zip.file("manifest.toml");
			if (manifestFile) {
				manifest = parseManifestToml(await manifestFile.async("string"));
			}

			const readmeFile = zip.file("README.md");
			if (readmeFile) {
				readme = await readmeFile.async("string");
			}

			for (const [zipPath, zipEntry] of Object.entries(zip.files)) {
				if (!zipEntry.dir && zipPath.startsWith("files/")) {
					let relativePath = zipPath.slice("files/".length);
					if (relativePath) {
						if (!relativePath.includes("/")) {
							relativePath = `${targetDirFor(relativePath)}/${relativePath}`;
						}
						files.push({
							path: relativePath,
							data: new Uint8Array(await zipEntry.async("arraybuffer")),
						});
					}
				}
			}

			const label = filePath.split(/[/\\]/).pop() ?? filePath;
			converter.sources = [...converter.sources, { label, files, manifest, readme }];

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
			if (readme && !converter.readmeContent) {
				converter.readmeContent = readme;
			}
		} catch (error) {
			console.error("Failed to parse V1 mod:", error);
		}
	}

	async function processLooseFile(filePath: string) {
		const data = await readFile(filePath);
		const fileName = filePath.split(/[/\\]/).pop() ?? filePath;
		converter.sources = [
			...converter.sources,
			{ label: fileName, files: [{ path: `${targetDirFor(fileName)}/${fileName}`, data }] },
		];
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

	function parseManifestToml(text: string): SourceEntry["manifest"] {
		const result: SourceEntry["manifest"] = { name: "", id: "", version: "1.0.0", authors: [] };
		const lines = text.split("\n");
		let inAuthors = false;
		let currentAuthor: { name: string; link: string } | null = null;

		for (const raw of lines) {
			const line = raw.trim();
			if (!line || line.startsWith("#")) continue;
			if (line === "[[mod.authors]]") {
				if (currentAuthor) result.authors!.push(currentAuthor);
				currentAuthor = { name: "", link: "" };
				inAuthors = true;
				continue;
			}
			const eqIdx = line.indexOf("=");
			if (eqIdx === -1) continue;
			const key = line.slice(0, eqIdx).trim();
			let val = line.slice(eqIdx + 1).trim();
			if (val.startsWith('"') && val.endsWith('"')) val = val.slice(1, -1);

			if (inAuthors) {
				if (key === "name") currentAuthor!.name = val;
				else if (key === "link") currentAuthor!.link = val;
			} else {
				if (key === "name") result.name = val;
				else if (key === "id") result.id = val;
				else if (key === "version") result.version = val;
			}
		}
		if (currentAuthor) result.authors!.push(currentAuthor);
		return result;
	}

	async function cook() {
		if (!converter.modName.trim()) return;
		cooking = true;
		converter.cookedZip = null;

		try {
			const zip = new JSZip();

			for (const source of converter.sources) {
				for (const file of source.files) {
					zip.file(`files/${file.path}`, file.data);
				}
			}

			const lines: string[] = [];
			lines.push("[mod]");
			lines.push(`id = "${converter.modName.trim()}"`);
			lines.push(`name = "${converter.modName.trim()}"`);
			lines.push(`version = "${converter.modVersion.trim() || "1.0.0"}"`);
			lines.push("");
			for (const author of converter.authors) {
				if (!author.name.trim()) continue;
				lines.push("[[mod.authors]]");
				lines.push(`name = "${author.name.trim()}"`);
				if (author.link.trim()) lines.push(`link = "${author.link.trim()}"`);
				lines.push("");
			}
			zip.file("manifest.toml", lines.join("\n"));

			if (converter.readmeContent.trim()) {
				zip.file("README.md", converter.readmeContent.trim());
			}

			converter.cookedZip = await zip.generateAsync({
				type: "uint8array",
				compression: "DEFLATE",
			});
		} catch (error) {
			console.error("Failed to cook mod:", error);
		} finally {
			cooking = false;
		}
	}

	async function download() {
		if (!converter.cookedZip) return;
		const savePath = await saveDialog({
			defaultPath: `${converter.modName.trim() || "converted-mod"}.tempest`,
			filters: [{ name: "Tempest Mod", extensions: ["tempest"] }],
		});
		if (!savePath) return;
		try {
			const { writeFile } = await import("@tauri-apps/plugin-fs");
			await writeFile(savePath, converter.cookedZip);
			addToast({
				title: m.converter_toast_saved_title(),
				message: m.converter_toast_saved_message({ path: savePath }),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to save mod:", error);
		}
	}

	async function installToInstance(inst: Instance) {
		if (!converter.cookedZip || !inst?.path) return;
		const modFileName = converter.modName.trim() || "converted-mod";
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

			const { writeFile } = await import("@tauri-apps/plugin-fs");
			const tmpDir = await path.tempDir();
			const tmpFile = await path.join(tmpDir, `${modFileName}.tempest`);
			await writeFile(tmpFile, converter.cookedZip);
			const result = await installMod(inst.path, tmpFile, true, true);

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
			try {
				await import("@tauri-apps/plugin-fs").then((m) => m.remove(tmpFile));
			} catch {}
		} catch (error) {
			if (installingToastId) removeToast(installingToastId);
			console.error("Failed to install mod:", error);
		} finally {
			installingLabel = null;
		}
	}

	function addToInstance() {
		if (!converter.cookedZip) return;
		const instanceId = window.location.pathname.match(/^\/instance\/([^/]+)/)?.[1];
		if (instanceId && instanceMap.value[instanceId]?.path) {
			void installToInstance(instanceMap.value[instanceId]);
		} else {
			showInstanceSelect = true;
		}
	}
</script>

{#if isDraggingConverterFiles.value}
	<div
		class="bg-base-100/90 pointer-events-none absolute inset-0 z-40 flex flex-col items-center justify-center backdrop-blur-sm"
		style="animation: fade-in 0.12s ease forwards;"
	>
		<div
			class="card bg-base-200/95 border-accent/50 w-full max-w-sm border-2 border-dashed text-center shadow-2xl"
			style="animation: pop-in 0.15s var(--ease-spring) forwards;"
		>
			<div class="card-body items-center gap-3">
				<div
					class="bg-accent/10 text-accent flex h-16 w-16 items-center justify-center rounded-xl"
				>
					<FlaskConical size={32} />
				</div>
				<h2 class="card-title justify-center">{m.converter_title()}</h2>
				<p class="text-sm opacity-70">
					{m.converter_drop_hint({
						tempest: ".tempest",
						upk: ".upk",
						pck: ".pck",
						ini: ".ini",
					})}
				</p>
			</div>
		</div>
	</div>
{/if}

<div class="bg-base-100 flex h-full flex-col">
	<Header title={m.converter_title()}>
		{#snippet icon()}
			<FlaskConical size={32} class="opacity-60" />
		{/snippet}
	</Header>

	<div class="bg-base-100 flex flex-1 flex-col overflow-hidden">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				<div class="mb-3 flex items-center justify-between">
					<span class="text-sm font-semibold">{m.converter_source_files()}</span>
					<button class="btn btn-accent btn-sm gap-2" onclick={pickFiles}>
						<Plus size={16} />
						{m.common_browse()}
					</button>
				</div>

				{#if converter.sources.length === 0}
					<div
						class="border-base-300 rounded-box mb-4 border-2 border-dashed p-12 text-center"
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
					</div>
				{:else}
					<div class="mb-2 space-y-2">
						{#each converter.sources as source, sIdx (sIdx)}
							<div class="bg-base-200 rounded-lg p-4">
								<div class="mb-2 flex items-center justify-between">
									<span class="truncate font-mono text-sm font-semibold"
										>{source.label}</span
									>
									<button
										class="btn btn-ghost btn-sm btn-square text-error"
										onclick={() => removeSource(sIdx)}
									>
										<Trash2 size={14} />
									</button>
								</div>
								<div class="space-y-1">
									{#each source.files as file, fIdx}
										<div class="pl-2">
											<input
												type="text"
												class="input input-ghost w-full min-w-0 font-mono"
												value={file.path}
												oninput={(e) =>
													updateTargetPath(
														sIdx,
														fIdx,
														e.currentTarget.value,
													)}
											/>
										</div>
									{/each}
								</div>
							</div>
						{/each}
					</div>
				{/if}
				{#if importingLabel}
					<div class="text-base-content/50 mb-6 flex items-center gap-2 text-xs">
						<span class="loading loading-spinner loading-xs"></span>
						{m.converter_importing({ name: importingLabel })}
					</div>
				{:else}
					<p class="text-base-content/50 mb-6 text-xs">
						{totalFiles}
						{m.downloads_files({ count: totalFiles })} across {converter.sources.length}
						{m.converter_sources({ count: converter.sources.length })}
					</p>
				{/if}

				<div class="divider my-0 mb-6"></div>

				<div class="flex flex-col gap-4">
					<div class="flex flex-col gap-1">
						<span class="text-sm font-semibold">{m.converter_mod_name_label()}</span>
						<input
							id="conv-name"
							type="text"
							class="input input-bordered w-full"
							bind:value={converter.modName}
							placeholder={m.converter_mod_name_placeholder()}
						/>
					</div>

					<div class="flex w-32 flex-col gap-1">
						<span class="text-sm font-semibold">{m.common_version()}</span>
						<input
							id="conv-version"
							type="text"
							class="input input-bordered w-full"
							bind:value={converter.modVersion}
							placeholder={m.converter_version_placeholder()}
						/>
					</div>
				</div>

				<div class="divider my-6"></div>

				<div class="form-control">
					<div class="mb-3 flex items-center justify-between">
						<span class="text-sm font-semibold">{m.converter_authors_label()}</span>
						<button class="btn btn-accent btn-sm gap-2" onclick={addAuthor}>
							<Plus size={16} />
							{m.common_add()}
						</button>
					</div>
					<div class="space-y-2">
						{#each converter.authors as author, i}
							<div class="bg-base-200 rounded-lg p-4">
								<div class="flex flex-col gap-2">
									<input
										type="text"
										class="input input-bordered w-full"
										placeholder={m.converter_author_name_placeholder()}
										value={author.name}
										oninput={(e) =>
											updateAuthor(i, "name", e.currentTarget.value)}
									/>
									<div class="flex items-center gap-2">
										<input
											type="text"
											class="input input-bordered flex-1"
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

				<div class="divider my-6"></div>

				<div class="form-control mb-6">
					<span class="mb-2 block text-sm font-semibold"
						>{m.converter_readme_label()}</span
					>
					<textarea
						id="conv-readme"
						class="textarea textarea-bordered h-48 w-full"
						bind:value={converter.readmeContent}
						placeholder={m.converter_readme_placeholder()}></textarea>
				</div>

				<div class="flex items-center justify-between">
					<button
						class="btn btn-accent gap-2"
						disabled={!canCook || cooking}
						onclick={cook}
					>
						{#if cooking}
							<span class="loading loading-spinner loading-xs"></span>
							{m.converter_cooking()}
						{:else}
							<FlaskConical size={18} />
							{m.converter_cook()}
						{/if}
					</button>

					{#if converter.cookedZip}
						<div class="flex items-center gap-2">
							<button class="btn btn-accent gap-2" onclick={download}>
								<FolderOpen size={16} />
								{m.converter_download()}
							</button>
							<button class="btn btn-accent gap-2" onclick={addToInstance}>
								<File size={16} />
								{m.converter_add_to_instance()}
							</button>
						</div>
					{/if}
				</div>
			</div>
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
