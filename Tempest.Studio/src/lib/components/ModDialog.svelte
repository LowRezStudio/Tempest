<script lang="ts">
	import { Trash2 } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { invoke, pickDirectory, pickFiles, PACKAGE_FILTERS } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { DeltaStatus, ModDirEntry } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let id = $state("");
	let name = $state("");
	let version = $state("1.0.0");
	let author = $state("");
	let files = $state<string[]>([]);
	let outDir = $state("");
	let done = $state<string | null>(null);
	let entries = $state<ModDirEntry[]>([]);
	/** Whether each file's base is recorded (so it ships as a delta), by path. */
	let status = $state<Record<string, DeltaStatus>>({});
	let recording = $state<string | null>(null);

	$effect(() => {
		if (open) {
			done = null;
			entries = [];
			if (files.length === 0 && studio.pkg) files = [studio.pkg.path];
		}
	});

	// The base record decides how a file ships; look it up for every listed file.
	$effect(() => {
		for (const f of files) {
			if (status[f]) continue;
			void rpc("delta.status", { path: f }).then(
				(s) => (status = { ...status, [f]: s }),
				() => (status = { ...status, [f]: { recorded: false } }),
			);
		}
	});

	async function addFiles() {
		const picked = await pickFiles("Add package files", [
			...PACKAGE_FILTERS,
			{ name: "Texture cache", extensions: ["tfc"] },
			{ name: "Assembly / language data", extensions: ["dat"] },
		]);
		files = [...new Set([...files, ...picked])];
	}

	async function pickOut() {
		const d = await pickDirectory("Mod folder (created if missing)");
		if (d) outDir = d;
	}

	/**
	 * Take the file as it is now as its base: for a file edited before the Studio kept records, or after restoring the original.
	 * @param f Path of the file.
	 */
	async function recordBase(f: string) {
		recording = f;
		try {
			const s = await rpc("delta.record", { path: f });
			status = { ...status, [f]: s };
			toasts.push("success", `${basename(f)}: this version is now the base`, "Base recorded");
		} catch (error) {
			toasts.error(error, "Could not record the base");
		} finally {
			recording = null;
		}
	}

	async function create(event: Event) {
		event.preventDefault();
		if (!id.trim() || !outDir || files.length === 0) return;
		try {
			const r = await rpc("mod.dir", {
				dir: outDir,
				id: id.trim(),
				name: name.trim() || undefined,
				version: version.trim() || undefined,
				author: author.trim() || undefined,
				files,
			});
			done = r.dir;
			entries = r.entries;
			const deltas = r.entries.filter((e) => e.mode === "delta").length;
			toasts.push(
				"success",
				`${r.files.length} files written${deltas ? `, ${deltas} as deltas` : ""}`,
				"Mod folder ready",
			);
		} catch (error) {
			toasts.error(error, "Could not create the mod folder");
		}
	}

	function kb(n: number) {
		return n >= 1024 * 1024
			? `${(n / 1024 / 1024).toFixed(1)} MB`
			: `${(n / 1024).toFixed(1)} KB`;
	}
</script>

<Modal bind:open title="Prepare a mod folder" width="max-w-xl">
	<form onsubmit={create} class="space-y-3 text-sm">
		<p class="text-xs opacity-60">
			Creates <code>manifest.toml</code> and the chosen files, ready for
			<code>tempest mod pack</code> /
			<code>just mod-pack &lt;dir&gt; &lt;out.tempest&gt;</code>, which packages and signs it.
			A package or <code>.dat</code> file saved in place from the Studio ships as a
			<em>delta</em>: only the objects or rows that changed, applied over the game's file by
			the launcher, so mods editing different parts of one file work together. A file without
			a recorded base is copied whole. Save your edited packages first.
		</p>
		<div class="grid grid-cols-2 gap-2">
			<label class="block"
				><span class="text-xs opacity-70">Mod id</span><input
					class="input input-sm mt-1 w-full font-mono"
					bind:value={id}
					placeholder="my-skin-pack"
					required
				/></label
			>
			<label class="block"
				><span class="text-xs opacity-70">Display name</span><input
					class="input input-sm mt-1 w-full"
					bind:value={name}
					placeholder="My skin pack"
				/></label
			>
			<label class="block"
				><span class="text-xs opacity-70">Version</span><input
					class="input input-sm mt-1 w-full font-mono"
					bind:value={version}
				/></label
			>
			<label class="block"
				><span class="text-xs opacity-70">Author</span><input
					class="input input-sm mt-1 w-full"
					bind:value={author}
				/></label
			>
		</div>
		<div>
			<div class="flex items-center justify-between">
				<span class="text-xs opacity-70">Files</span>
				<button class="btn btn-ghost btn-xs" type="button" onclick={addFiles}>Add…</button>
			</div>
			<ul class="rounded-box bg-base-200 mt-1 max-h-40 overflow-auto p-1 font-mono text-xs">
				{#each files as f, i (i)}
					<li class="flex items-center gap-2 px-1 py-0.5">
						<span class="truncate" title={f}>{basename(f)}</span>
						{#if studio.dirty.includes(f)}<span class="badge badge-xs badge-warning"
								>unsaved</span
							>{/if}
						{#if status[f]?.recorded}
							<span
								class="badge badge-xs badge-success"
								title="Base recorded{status[f].when
									? ` on ${new Date(status[f].when * 1000).toLocaleString()}`
									: ''}: ships as a delta">delta</span
							>
						{:else if status[f]}
							<span
								class="badge badge-xs"
								title="No base recorded: the whole file is copied">whole file</span
							>
							<button
								class="btn btn-ghost btn-xs"
								type="button"
								disabled={recording !== null}
								title="Take this file as it is now as the base, so later edits ship as a delta (only if it is the game's original)"
								onclick={() => recordBase(f)}
								>{recording === f ? "recording…" : "record base"}</button
							>
						{/if}
						<button
							class="btn btn-square btn-ghost btn-xs ml-auto"
							type="button"
							onclick={() => (files = files.filter((x) => x !== f))}
							aria-label="Remove"><Trash2 size={12} /></button
						>
					</li>
				{:else}
					<li class="px-1 py-0.5 opacity-60">no files yet</li>
				{/each}
			</ul>
		</div>
		<div class="flex gap-1">
			<input
				class="input input-sm flex-1 font-mono text-xs"
				bind:value={outDir}
				placeholder="output folder"
			/>
			<button class="btn btn-sm" type="button" onclick={pickOut}>Browse…</button>
		</div>
		{#if done}
			<div class="alert alert-success text-xs">
				<span
					>Ready in <code>{done}</code>. Package and sign it with
					<code>just mod-pack "{done}" out.tempest</code>.</span
				>
				<button
					class="btn btn-xs"
					type="button"
					onclick={() => invoke("shell:open-path", { path: done! })}>Open</button
				>
			</div>
			<ul class="bg-base-200 rounded-box p-2 font-mono text-[11px]">
				{#each entries as e (e.file)}
					<li>
						{e.file}:
						{#if e.mode === "delta"}<span class="text-success"
								>{e.ops} changed objects, {kb(e.bytes)}</span
							>{:else if e.mode === "copy"}whole file, {kb(e.bytes)}{:else}unchanged,
							skipped{/if}
						{#each e.warnings as w, i (i)}<div class="text-warning">{w}</div>{/each}
					</li>
				{/each}
			</ul>
		{/if}
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Close</button
			>
			<button
				class="btn btn-sm btn-primary"
				type="submit"
				disabled={!id.trim() || !outDir || files.length === 0}>Create</button
			>
		</div>
	</form>
</Modal>
