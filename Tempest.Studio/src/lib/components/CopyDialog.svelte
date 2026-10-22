<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickFile, PACKAGE_FILTERS } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ExportRow } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false), row }: { open?: boolean; row: ExportRow } = $props();

	let dst = $state("");
	let outerMode = $state<"same" | "top" | "path">("same");
	let outerPath = $state("");
	let deep = $state(true);
	let working = $state(false);

	const candidates = $derived(
		studio.packages.filter((p) => p.path !== studio.pkg?.path && !p.script),
	);

	async function browse() {
		const f = await pickFile("Destination package", PACKAGE_FILTERS, studio.workspace?.dir);
		if (f) dst = f;
	}

	async function submit(event: Event) {
		event.preventDefault();
		if (!studio.pkg || !dst) return;
		working = true;
		try {
			let outer: number | "top" | undefined;
			if (outerMode === "top") outer = "top";
			else if (outerMode === "path") {
				const tables = await rpc("package.open", { path: dst });
				const found = tables.exports.find(
					(e) => e.path.toLowerCase() === outerPath.trim().toLowerCase(),
				);
				if (!found) throw new Error(`no export ${outerPath} in ${basename(dst)}`);
				outer = found.i;
			}
			const r = await studio.run(`Copying ${row.name} into ${basename(dst)}…`, () =>
				rpc("object.copy", { src: studio.pkg!.path, index: row.i, dst, outer, deep }),
			);
			for (const w of r.warnings) toasts.push("warning", w, "Copy");
			studio.markDirty(dst);
			await studio.refreshPackages();
			toasts.push(
				"success",
				`${r.path} (export #${r.index}) in ${basename(dst)} — save that package to keep it`,
				"Copied",
			);
			open = false;
			await studio.openPackage(dst, r.index);
		} catch (error) {
			toasts.error(error, "Copy failed");
		} finally {
			working = false;
		}
	}
</script>

<Modal bind:open title="Copy {row.name} to another package" width="max-w-xl">
	<form onsubmit={submit} class="space-y-3 text-sm">
		<label class="block">
			<span class="text-xs opacity-70">Destination package</span>
			<div class="mt-1 flex gap-1">
				<input
					class="input input-sm flex-1 font-mono text-xs"
					list="copy-candidates"
					bind:value={dst}
					placeholder="path to a .upk"
				/>
				<datalist id="copy-candidates">
					{#each candidates as p (p.path)}
						<option value={p.path}>{p.name}</option>
					{/each}
				</datalist>
				<button class="btn btn-sm" type="button" onclick={browse}>Browse…</button>
			</div>
		</label>
		<fieldset class="space-y-1">
			<legend class="text-xs opacity-70">Place it under</legend>
			<label class="flex items-center gap-2"
				><input type="radio" class="radio radio-xs" bind:group={outerMode} value="same" /> the
				same package group path as the source (created if missing)</label
			>
			<label class="flex items-center gap-2"
				><input type="radio" class="radio radio-xs" bind:group={outerMode} value="top" /> the
				package root</label
			>
			<label class="flex items-center gap-2">
				<input type="radio" class="radio radio-xs" bind:group={outerMode} value="path" /> an
				existing object:
				<input
					class="input input-xs flex-1 font-mono"
					bind:value={outerPath}
					placeholder="group.subgroup"
					disabled={outerMode !== "path"}
				/>
			</label>
		</fieldset>
		<label class="flex items-center gap-2">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={deep} />
			Copy everything it references too (textures, parents, sub-objects); otherwise references become
			imports.
		</label>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Cancel</button
			>
			<button class="btn btn-sm btn-primary" type="submit" disabled={!dst || working}
				>Copy</button
			>
		</div>
	</form>
</Modal>
