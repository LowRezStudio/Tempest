<script lang="ts">
	import { CircleCheck, CircleX } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { pickDirectory } from "$lib/platform/api";
	import { onUpkEvent, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { BatchResult } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let dir = $state("");
	let recursive = $state(false);
	let result = $state<BatchResult | null>(null);
	let working = $state(false);
	let progress = $state<{ done: number; total: number; file: string } | null>(null);
	/** Package files the plan would write, unselected ones excluded from the run. */
	let packages = $state<{ path: string; name: string; files: number; on: boolean }[]>([]);
	const selectedPaths = $derived(packages.filter((p) => p.on).map((p) => p.path));
	const selectedFiles = $derived(packages.filter((p) => p.on).reduce((n, p) => n + p.files, 0));
	const unselected = $derived(new Set(packages.filter((p) => !p.on).map((p) => p.path)));

	/** The distinct packages of a plan, all selected. */
	function packagesOf(r: BatchResult) {
		const map = new Map<string, { path: string; name: string; files: number; on: boolean }>();
		for (const e of r.entries) {
			if (!e.ok || !e.packagePath) continue;
			const cur = map.get(e.packagePath);
			if (cur) {
				cur.files += 1;
			} else {
				map.set(e.packagePath, {
					path: e.packagePath,
					name: e.package,
					files: 1,
					on: true,
				});
			}
		}
		return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
	}

	function selectAll(on: boolean) {
		packages = packages.map((p) => ({ ...p, on }));
	}

	$effect(() => {
		if (!open) return;
		result = null;
		const stop = onUpkEvent((e) => {
			if (e.event === "batch.progress") {
				progress = {
					done: e.done as number,
					total: e.total as number,
					file: e.file as string,
				};
			}
		});
		return stop;
	});

	async function browse() {
		const d = await pickDirectory(
			"Folder with <package>.<object>.<ext> files",
			dir || studio.workspace?.dir,
		);
		if (d) {
			dir = d;
			await scan();
		}
	}

	async function scan() {
		if (!dir) return;
		working = true;
		try {
			result = await rpc("batch.plan", { dir, recursive });
			packages = packagesOf(result);
		} catch (error) {
			toasts.error(error, "Scan failed");
		} finally {
			working = false;
		}
	}

	/** Import into the open packages: unsaved edits (one undo step per package), saved like any other. */
	async function run() {
		if (!dir || !result || selectedFiles === 0) return;
		working = true;
		progress = null;
		try {
			const r = await studio.run("Importing…", () =>
				rpc("batch.run", { dir, recursive, packages: selectedPaths }),
			);
			result = r;
			const touched = [
				...new Set(
					r.entries.flatMap((e) => (e.ok && e.packagePath ? [e.packagePath] : [])),
				),
			];
			for (const path of touched) studio.markDirty(path);
			toasts.push(
				r.failed ? "warning" : "success",
				`${r.ok} imported${r.failed ? `, ${r.failed} failed` : ""}; ${touched.length} package${touched.length === 1 ? "" : "s"} unsaved until you save (or build the mod)`,
				"Batch import",
			);
			// The package on screen changed in memory: show it as it is now.
			if (studio.pkg && touched.includes(studio.pkg.path)) {
				await studio.reloadTables();
				if (studio.selected !== null) await studio.select(studio.selected);
			}
		} catch (error) {
			toasts.error(error, "Batch import failed");
		} finally {
			working = false;
			progress = null;
		}
	}
</script>

<Modal bind:open title="Batch import" width="max-w-4xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-60">
			Every file named <code>&lt;package&gt;.&lt;object&gt;.&lt;ext&gt;</code> (for example
			<code>loadingart_androxus.loadingart_androxus_antlers.dds</code>) replaces that object:
			textures from .png/.dds, meshes from .glb/.gltf/.fbx/.obj, sounds from .ogg, movies from
			.gfx/.swf, raw payloads from .bin. The first part is a package file of the workspace, or
			the object's top-level group as shown in the export tree: then the whole name is the
			object path and every package holding it is updated. The object part is a name (unique
			among objects the file type can replace) or a full path like
			<code>group.sub.name</code>.
		</p>
		<div class="flex gap-1">
			<input
				class="input input-sm flex-1 font-mono text-xs"
				bind:value={dir}
				placeholder="folder of files"
			/>
			<button class="btn btn-sm" type="button" onclick={browse}>Browse…</button>
			<button class="btn btn-sm" type="button" onclick={scan} disabled={!dir || working}
				>Scan</button
			>
		</div>
		<label class="flex items-center gap-2 text-xs">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={recursive} /> include subfolders
		</label>
		<p class="text-xs opacity-60">
			The imports are edits of the packages, like Replace: they stay unsaved (and in your
			project) until you save, can be undone, and go into the mod like any other edit.
		</p>

		{#if result && !result.ran && packages.length > 1}
			<fieldset class="border-base-300 rounded border p-2 text-xs">
				<legend class="flex items-center gap-2 px-1 opacity-70">
					Packages to change ({selectedPaths.length}/{packages.length})
					<button
						class="btn btn-ghost btn-xs"
						type="button"
						onclick={() => selectAll(true)}>Select all</button
					>
					<button
						class="btn btn-ghost btn-xs"
						type="button"
						onclick={() => selectAll(false)}>Deselect all</button
					>
				</legend>
				<div class="grid max-h-40 grid-cols-2 gap-x-4 gap-y-1 overflow-auto md:grid-cols-3">
					{#each packages as p (p.path)}
						<label class="flex items-center gap-2 truncate" title={p.path}>
							<input
								type="checkbox"
								class="checkbox checkbox-xs"
								bind:checked={p.on}
							/>
							<span class="truncate font-mono">{p.name}</span>
							<span class="opacity-50">{p.files}</span>
						</label>
					{/each}
				</div>
			</fieldset>
		{/if}
		{#if result}
			<div class="flex items-center gap-2">
				<span>
					<b>{result.ran ? result.ok : selectedFiles}</b> file{(result.ran
						? result.ok
						: selectedFiles) === 1
						? ""
						: "s"}
					{result.ran ? "imported" : "ready"}{result.failed
						? `, ${result.failed} skipped`
						: ""}{!result.ran && result.ok - selectedFiles > 0
						? `, ${result.ok - selectedFiles} unselected`
						: ""}
				</span>
				{#if progress && working}
					<span class="font-mono text-xs opacity-60"
						>{progress.done}/{progress.total} {progress.file}</span
					>
				{/if}
				<div class="flex-1"></div>
				{#if !result.ran}
					<button
						class="btn btn-sm btn-primary"
						onclick={run}
						disabled={working || selectedFiles === 0}
					>
						{#if working}<span class="loading loading-xs loading-spinner"></span>{/if}
						Import {selectedFiles} file{selectedFiles === 1 ? "" : "s"}
					</button>
				{/if}
			</div>
			<div class="max-h-[45vh] overflow-auto">
				<table class="table-xs table font-mono">
					<tbody>
						{#each result.entries as e, k (k)}
							<tr
								class={!e.ok ||
								(e.packagePath !== null && unselected.has(e.packagePath))
									? "opacity-50"
									: ""}
							>
								<td class="w-4">
									{#if e.ok}<CircleCheck
											size={13}
											class="text-success"
										/>{:else}<CircleX size={13} class="text-error" />{/if}
								</td>
								<td class="max-w-xs truncate" title={e.file}>{e.name}</td>
								<td class="opacity-70">{e.package}</td>
								<td class="selectable max-w-md truncate" title={e.message}
									>{e.message}</td
								>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</Modal>
