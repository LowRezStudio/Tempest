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
	let mode = $state<"inplace" | "folder">("folder");
	let outDir = $state("");
	let result = $state<BatchResult | null>(null);
	let working = $state(false);
	let progress = $state<{ done: number; total: number; file: string } | null>(null);

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

	async function browseOut() {
		const d = await pickDirectory("Folder for the modified packages", outDir || undefined);
		if (d) outDir = d;
	}

	async function scan() {
		if (!dir) return;
		working = true;
		try {
			result = await rpc("batch.plan", { dir, recursive });
		} catch (error) {
			toasts.error(error, "Scan failed");
		} finally {
			working = false;
		}
	}

	async function run() {
		if (!dir || !result || result.ok === 0) return;
		if (mode === "folder" && !outDir) return;
		working = true;
		progress = null;
		const openPath = studio.pkg?.path ?? null;
		const selected = studio.selected;
		try {
			const r = await studio.run("Importing…", () =>
				rpc("batch.run", {
					dir,
					recursive,
					outDir: mode === "folder" ? outDir : undefined,
				}),
			);
			result = r;
			toasts.push(
				r.failed ? "warning" : "success",
				`${r.ok} imported${r.failed ? `, ${r.failed} failed` : ""}${mode === "folder" ? ` → ${outDir}` : ""}`,
				"Batch import",
			);
			await studio.refreshPackages();
			// Packages saved in place changed on disk: reopen the one on screen.
			if (
				mode === "inplace" &&
				openPath &&
				r.entries.some((e) => e.ok && e.packagePath === openPath)
			) {
				await studio.openPackage(openPath, selected ?? undefined);
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
		<fieldset class="space-y-1 text-xs">
			<legend class="opacity-70">Write the modified packages</legend>
			<label class="flex items-center gap-2">
				<input type="radio" class="radio radio-xs" bind:group={mode} value="folder" /> into
				a folder (ready for a mod):
				<input
					class="input input-xs flex-1 font-mono"
					bind:value={outDir}
					placeholder="output folder"
					disabled={mode !== "folder"}
				/>
				<button
					class="btn btn-xs"
					type="button"
					onclick={browseOut}
					disabled={mode !== "folder"}>Browse…</button
				>
			</label>
			<label class="flex items-center gap-2">
				<input type="radio" class="radio radio-xs" bind:group={mode} value="inplace" /> in place,
				overwriting the game's packages
			</label>
		</fieldset>

		{#if result}
			<div class="flex items-center gap-2">
				<span>
					<b>{result.ok}</b> file{result.ok === 1 ? "" : "s"}
					{result.ran ? "imported" : "ready"}{result.failed
						? `, ${result.failed} skipped`
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
						disabled={working || result.ok === 0 || (mode === "folder" && !outDir)}
					>
						{#if working}<span class="loading loading-xs loading-spinner"></span>{/if}
						Import {result.ok} file{result.ok === 1 ? "" : "s"}
					</button>
				{/if}
			</div>
			<div class="max-h-[45vh] overflow-auto">
				<table class="table-xs table font-mono">
					<tbody>
						{#each result.entries as e, k (k)}
							<tr class={e.ok ? "" : "opacity-70"}>
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
