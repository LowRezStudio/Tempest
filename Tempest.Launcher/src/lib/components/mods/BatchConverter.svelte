<script lang="ts">
	import { FileUp, FlaskConical, FolderOpen, FolderDown, Trash2 } from "@lucide/svelte";
	import { describeError } from "$lib/core/mods";
	import { converterPendingPaths } from "$lib/mods/drop.svelte";
	import { MOD_ID_PATTERN, buildPackageZip } from "$lib/mods/packaging";
	import { signWithPrompt } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openPath } from "$lib/platform/api";
	import { batch, type BatchEntry } from "$lib/stores/batch.svelte";
	import { addToast } from "$lib/stores/ui.svelte";

	let cooking = $state(false);
	let cookedCount = $state(0);
	let cookTotal = $state(0);
	let saving = $state(false);
	let lastSavedDir = $state<string | null>(null);

	let queuedEntries = $derived(
		batch.entries.filter(
			(e) => e.files.length > 0 && MOD_ID_PATTERN.test(e.id.trim()) && e.status !== "signed",
		),
	);
	let signedEntries = $derived(
		batch.entries.filter((e) => e.status === "signed" && e.stagedPath),
	);
	let canCookAll = $derived(queuedEntries.length > 0 && !cooking);
	let canSaveAll = $derived(signedEntries.length > 0 && !cooking);

	$effect(() => {
		if (converterPendingPaths.length > 0) {
			const paths = [...converterPendingPaths];
			converterPendingPaths.length = 0;
			void (async () => {
				const imports = await invoke("converter:read-imports", { paths });
				const skipped = paths.length - imports.length;
				if (skipped > 0) {
					addToast({
						title: m.toast_unsupported_file_title(),
						message: m.batch_tempest_only(),
						tone: "error",
					});
				}
				for (const imported of imports) {
					await addTempest(imported.name, new Uint8Array(imported.data));
				}
			})();
		}
	});

	async function pickFiles() {
		// Dialog + read both happen on the host in one round trip.
		const imports = await invoke("converter:pick-imports");
		let skipped = 0;
		for (const imported of imports) {
			if (!imported.name.toLowerCase().endsWith(".tempest")) {
				skipped++;
				continue;
			}
			await addTempest(imported.name, new Uint8Array(imported.data));
		}
		if (skipped > 0) {
			addToast({
				title: m.toast_unsupported_file_title(),
				message: m.batch_tempest_only(),
				tone: "error",
			});
		}
	}

	async function addTempest(name: string, data: Uint8Array) {
		const compressed = await batch.add(name, data);
		if (compressed === null) {
			addToast({
				title: m.converter_cook_failed(),
				message: name,
				tone: "error",
			});
			return;
		}
		if (compressed > 0) {
			addToast({
				title: name,
				message: m.converter_upk_optimized({ name }),
				tone: "success",
			});
		}
	}

	async function cookAll() {
		if (cooking) return;
		cooking = true;
		cookedCount = 0;
		let signed = 0;
		// Snapshot: the derived queue shrinks as entries become signed.
		const targets = [...queuedEntries];
		cookTotal = targets.length;
		try {
			for (const entry of targets) {
				entry.status = "cooking";
				entry.error = null;
				try {
					const filename = `${entry.id.trim()}-${entry.version.trim() || "1.0.0"}.tempest`;
					const { bytes, duplicates } = await buildPackageZip(
						{
							id: entry.id.trim(),
							name: entry.name.trim() || entry.id.trim(),
							version: entry.version.trim() || "1.0.0",
							authors: entry.authors,
							readme: entry.readme,
						},
						entry.files,
					);
					if (duplicates > 0) {
						addToast({
							title: m.converter_source_files(),
							message: m.converter_duplicates_skipped(),
							tone: "warning",
						});
					}
					const stagedPath = await invoke("mods:stage-pack", { filename, data: bytes });
					const previous = entry.stagedPath;
					entry.signer = (await signWithPrompt(stagedPath)) || null;
					entry.stagedPath = stagedPath;
					entry.status = "signed";
					if (previous && previous !== stagedPath) {
						void invoke("mods:discard-staged-pack", { stagedPath: previous }).catch(
							() => {},
						);
					}
					signed++;
				} catch (error) {
					entry.status = "failed";
					entry.error = describeError(error);
				}
				cookedCount++;
			}
		} finally {
			cooking = false;
			if (signed > 0) {
				addToast({
					title: m.batch_toast_signed_title(),
					message: m.batch_toast_signed_message({ signed, total: targets.length }),
					tone: "success",
				});
			}
		}
	}

	async function saveAll() {
		if (saving || signedEntries.length === 0) return;
		saving = true;
		try {
			const result = await invoke("mods:save-staged-batch", {
				files: signedEntries.map((e) => ({
					stagedPath: e.stagedPath!,
					filename: `${e.id.trim()}-${e.version.trim() || "1.0.0"}.tempest`,
				})),
			});
			if (!result) return;
			lastSavedDir = result.dir;
			addToast({
				title: m.batch_saved_title(),
				message: m.batch_saved_message({ count: result.saved.length, dir: result.dir }),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to save cooked mods:", error);
			addToast({
				title: m.batch_cook_failed_title(),
				message: describeError(error),
				tone: "error",
			});
		} finally {
			saving = false;
		}
	}

	function statusBadge(entry: BatchEntry) {
		switch (entry.status) {
			case "cooking": {
				return {
					class: "badge-info",
					text: m.batch_cooking({ done: cookedCount, total: cookTotal }),
				};
			}
			case "signed": {
				return { class: "badge-success", text: m.batch_status_signed() };
			}
			case "failed": {
				return { class: "badge-error", text: m.batch_status_failed() };
			}
			default: {
				return { class: "badge-ghost", text: m.batch_status_queued() };
			}
		}
	}
</script>

<div class="h-full overflow-y-auto">
	<div class="mx-auto w-full max-w-6xl px-4 py-6">
		<section class="card bg-base-200">
			<div class="card-body gap-4 p-4">
				<div class="flex items-center justify-between">
					<h2 class="text-sm font-semibold tracking-wide uppercase">
						{m.batch_queue_section()}
					</h2>
					<div class="flex items-center gap-2">
						{#if batch.entries.length > 0}
							<button
								class="btn btn-ghost btn-sm text-error"
								onclick={() => batch.clear()}
							>
								<Trash2 size={14} />
								{m.common_reset()}
							</button>
						{/if}
						<button class="btn btn btn-sm gap-2" onclick={pickFiles}>
							<FileUp size={16} />
							{m.common_browse()}
						</button>
					</div>
				</div>

				{#if batch.entries.length === 0}
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
							<p class="text-xs opacity-60">{m.batch_tempest_only()}</p>
						</div>
					</button>
				{:else}
					<div class="space-y-2">
						{#each batch.entries as entry (entry.key)}
							<div class="bg-base-300/50 rounded-lg p-3">
								<div class="flex items-center justify-between gap-2">
									<span class="truncate font-mono text-sm font-semibold"
										>{entry.label}</span
									>
									<div class="flex shrink-0 items-center gap-2">
										{#if entry.status === "cooking"}
											<span class="loading loading-spinner loading-xs"></span>
										{/if}
										<span class="badge badge-sm {statusBadge(entry).class}">
											{statusBadge(entry).text}
										</span>
										<button
											class="btn btn-ghost btn-sm btn-square text-error"
											disabled={entry.status === "cooking"}
											onclick={() => batch.remove(entry.key)}
										>
											<Trash2 size={14} />
										</button>
									</div>
								</div>
								<div class="mt-2 grid gap-2 sm:grid-cols-[1fr_7rem]">
									<label class="form-control gap-1">
										<span class="text-base-content/50 text-xs">
											{m.converter_mod_id_label()}
										</span>
										<input
											type="text"
											class="input input-bordered input-sm font-mono"
											class:input-error={entry.id.trim() !== "" &&
												!MOD_ID_PATTERN.test(entry.id.trim())}
											bind:value={entry.id}
											placeholder={m.converter_mod_id_placeholder()}
											spellcheck="false"
										/>
									</label>
									<label class="form-control gap-1">
										<span class="text-base-content/50 text-xs"
											>{m.common_version()}</span
										>
										<input
											type="text"
											class="input input-bordered input-sm"
											bind:value={entry.version}
											placeholder={m.converter_version_placeholder()}
										/>
									</label>
								</div>
								{#if entry.error}
									<p class="text-error mt-1 text-xs">{entry.error}</p>
								{:else if entry.status === "signed" && entry.signer}
									<p class="text-success mt-1 font-mono text-xs">
										{entry.signer}
									</p>
								{:else if entry.id.trim() !== "" && !MOD_ID_PATTERN.test(entry.id.trim())}
									<p class="text-error mt-1 text-xs">
										{m.converter_mod_id_invalid()}
									</p>
								{/if}
								{#if entry.compressedFiles > 0}
									<p class="text-base-content/50 mt-1 text-xs">
										{m.batch_files_compressed({ count: entry.compressedFiles })}
									</p>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</section>

		<div class="from-base-100 sticky bottom-0 mt-4 bg-gradient-to-t from-60% pt-4 pb-2">
			<div class="flex flex-wrap items-center gap-2">
				<button
					class="btn btn-primary btn-lg gap-2"
					disabled={!canCookAll}
					onclick={cookAll}
				>
					{#if cooking}
						<span class="loading loading-spinner loading-xs"></span>
						{m.batch_cooking({ done: cookedCount, total: cookTotal })}
					{:else}
						<FlaskConical size={20} />
						{m.batch_cook_all()}
					{/if}
				</button>

				{#if canSaveAll}
					<button
						class="btn btn-primary btn-lg gap-2"
						disabled={saving}
						onclick={saveAll}
					>
						{#if saving}
							<span class="loading loading-spinner loading-xs"></span>
							{m.batch_saving()}
						{:else}
							<FolderDown size={20} />
							{m.batch_save_all()}
						{/if}
					</button>
				{/if}

				{#if lastSavedDir}
					<button
						class="btn btn-ghost btn-lg gap-2"
						onclick={() => lastSavedDir && openPath(lastSavedDir)}
					>
						<FolderOpen size={20} />
						{m.batch_open_folder()}
					</button>
				{/if}
			</div>
		</div>
	</div>
</div>
