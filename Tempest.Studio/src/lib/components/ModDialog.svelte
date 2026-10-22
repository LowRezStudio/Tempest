<script lang="ts">
	import { Trash2 } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { invoke, pickDirectory, pickFiles, PACKAGE_FILTERS } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let id = $state("");
	let name = $state("");
	let version = $state("1.0.0");
	let author = $state("");
	let files = $state<string[]>([]);
	let outDir = $state("");
	let done = $state<string | null>(null);

	$effect(() => {
		if (open) {
			done = null;
			if (files.length === 0 && studio.pkg) files = [studio.pkg.path];
		}
	});

	async function addFiles() {
		const picked = await pickFiles("Add package files", [
			...PACKAGE_FILTERS,
			{ name: "Texture cache", extensions: ["tfc"] },
		]);
		files = [...new Set([...files, ...picked])];
	}

	async function pickOut() {
		const d = await pickDirectory("Mod folder (created if missing)");
		if (d) outDir = d;
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
			toasts.push("success", `${r.files.length} files copied`, "Mod folder ready");
		} catch (error) {
			toasts.error(error, "Could not create the mod folder");
		}
	}
</script>

<Modal bind:open title="Prepare a mod folder" width="max-w-xl">
	<form onsubmit={create} class="space-y-3 text-sm">
		<p class="text-xs opacity-60">
			Creates <code>manifest.toml</code> and <code>files/</code> with the chosen packages,
			ready for
			<code>tempest mod pack</code> /
			<code>just mod-pack &lt;dir&gt; &lt;out.tempest&gt;</code>, which packages and signs it.
			Save your edited packages first.
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
				{#each files as f (f)}
					<li class="flex items-center gap-2 px-1 py-0.5">
						<span class="truncate" title={f}>{basename(f)}</span>
						{#if studio.dirty.includes(f)}<span class="badge badge-xs badge-warning"
								>unsaved</span
							>{/if}
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
