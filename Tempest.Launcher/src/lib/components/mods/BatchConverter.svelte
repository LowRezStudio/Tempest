<script lang="ts">
	import {
		Circle,
		CircleCheck,
		FileArchive,
		FileUp,
		FolderDown,
		FolderOpen,
		ListChecks,
		Plus,
		RotateCcw,
		X,
		XCircle,
	} from "@lucide/svelte";
	import { describeError } from "$lib/core/mods";
	import { converterPendingPaths, isDraggingConverterFiles } from "$lib/mods/drop.svelte";
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
	let confirmReset = $state(false);

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
	const tally = $derived([
		{
			label: m.batch_status_queued(),
			icon: Circle,
			class: "text-primary",
			count: batch.entries.filter((e) => e.status === "queued" || e.status === "cooking")
				.length,
		},
		{
			label: m.batch_status_signed(),
			icon: CircleCheck,
			class: "text-success",
			count: batch.entries.filter((e) => e.status === "signed").length,
		},
		{
			label: m.batch_status_failed(),
			icon: XCircle,
			class: "text-error",
			count: batch.entries.filter((e) => e.status === "failed").length,
		},
	]);

	/** Empties the queue; asks once more first. */
	function startOver() {
		if (!confirmReset) {
			confirmReset = true;
			setTimeout(() => (confirmReset = false), 3000);
			return;
		}
		confirmReset = false;
		batch.clear();
		lastSavedDir = null;
	}

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
		let compressed: number;
		try {
			compressed = await batch.add(name, data);
		} catch (error) {
			console.error("Failed to parse mod package:", name, error);
			addToast({
				title: m.converter_import_failed({ name }),
				message: describeError(error),
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
							icon: entry.icon,
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

<div class="@container h-full overflow-y-auto">
	<div
		class="mx-auto grid w-full max-w-6xl items-start gap-4 px-4 py-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]"
	>
		<section class="bg-base-200 border-base-300 rounded-box min-w-0 border p-4">
			<div class="mb-3 flex min-h-6 items-center gap-2">
				<span
					class="bg-base-300 flex size-5 shrink-0 items-center justify-center rounded-full"
					><ListChecks size={12} /></span
				>
				<h2 class="text-sm font-semibold">{m.batch_queue_section()}</h2>
				{#if batch.entries.length > 0}
					<span class="text-base-content/50 text-xs">{batch.entries.length}</span>
				{/if}
				<button type="button" class="btn btn-ghost btn-xs ml-auto" onclick={pickFiles}>
					<Plus size={12} />
					{m.common_browse()}
				</button>
			</div>

			{#if batch.entries.length === 0}
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
						{m.converter_drop_hint_packages({ tempest: ".tempest" })}
					</span>
					<span class="text-base-content/60 text-sm"
						>{m.converter_drop_browse_hint()}</span
					>
				</button>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each batch.entries as entry (entry.key)}
						{@const idInvalid =
							entry.id.trim() !== "" && !MOD_ID_PATTERN.test(entry.id.trim())}
						<li class="bg-base-100 border-base-300 rounded-field border p-3">
							<div class="flex items-center gap-3">
								<FileArchive size={20} class="text-primary shrink-0" />
								<p class="min-w-0 flex-1 truncate font-mono text-sm">
									{entry.label}
								</p>
								{#if entry.status === "cooking"}
									<span class="loading loading-spinner loading-xs"></span>
								{/if}
								<span class="badge badge-sm shrink-0 {statusBadge(entry).class}">
									{statusBadge(entry).text}
								</span>
								<button
									type="button"
									class="btn btn-ghost btn-sm btn-square"
									aria-label={m.common_remove()}
									disabled={entry.status === "cooking"}
									onclick={() => batch.remove(entry.key)}
								>
									<X size={14} />
								</button>
							</div>
							<div class="mt-2 grid grid-cols-[minmax(0,1fr)_7rem] gap-2">
								<label class="flex flex-col gap-1">
									<span class="text-base-content/60 text-xs">
										{m.converter_mod_id_label()}
									</span>
									<input
										class={[
											"input input-sm w-full font-mono",
											idInvalid && "input-error",
										]}
										bind:value={entry.id}
										placeholder={m.converter_mod_id_placeholder()}
										spellcheck="false"
									/>
								</label>
								<label class="flex flex-col gap-1">
									<span class="text-base-content/60 text-xs"
										>{m.common_version()}</span
									>
									<input
										class="input input-sm w-full font-mono"
										bind:value={entry.version}
										placeholder={m.converter_version_placeholder()}
									/>
								</label>
							</div>
							{#if entry.error}
								<p class="text-error mt-2 text-xs">{entry.error}</p>
							{:else if entry.status === "signed" && entry.signer}
								<p class="text-base-content/60 mt-2 font-mono text-xs">
									{m.converter_signed_by({ signer: entry.signer })}
								</p>
							{:else if idInvalid}
								<p class="text-error mt-2 text-xs">
									{m.converter_mod_id_invalid()}
								</p>
							{/if}
							{#if entry.compressedFiles > 0}
								<p class="text-base-content/60 mt-1 text-xs">
									{m.batch_files_compressed({ count: entry.compressedFiles })}
								</p>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
			<p class="text-base-content/60 mt-2 text-xs">{m.batch_tempest_only()}</p>
		</section>

		<aside class="flex flex-col gap-3 @3xl:sticky @3xl:top-6">
			<div class="bg-base-200 border-base-300 rounded-box flex flex-col gap-3 border p-4">
				<ul class="flex flex-col gap-0.5 text-sm">
					{#each tally as row (row.label)}
						<li class="flex items-center gap-2 px-2 py-1">
							<row.icon size={14} class={row.class} />
							{row.label}
							<span class="text-base-content/60 ml-auto font-mono text-xs"
								>{row.count}</span
							>
						</li>
					{/each}
				</ul>
				<button
					type="button"
					class="btn btn-primary w-full"
					disabled={!canCookAll}
					onclick={cookAll}
				>
					{#if cooking}
						<span class="loading loading-spinner loading-xs"></span>
						{m.batch_cooking({ done: cookedCount, total: cookTotal })}
					{:else}
						{m.batch_cook_all()}
					{/if}
				</button>
				{#if cooking}
					<progress
						class="progress progress-primary w-full"
						value={cookedCount}
						max={cookTotal}
					></progress>
				{/if}
				{#if canSaveAll}
					<button type="button" class="btn w-full" disabled={saving} onclick={saveAll}>
						{#if saving}
							<span class="loading loading-spinner loading-xs"></span>
							{m.batch_saving()}
						{:else}
							<FolderDown size={16} />
							{m.batch_save_all()}
						{/if}
					</button>
				{/if}
				{#if lastSavedDir}
					<button
						type="button"
						class="btn btn-ghost w-full"
						onclick={() => lastSavedDir && openPath(lastSavedDir)}
					>
						<FolderOpen size={16} />
						{m.batch_open_folder()}
					</button>
				{/if}
				{#if batch.entries.length > 0}
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
		</aside>
	</div>
</div>
