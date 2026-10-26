<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { invoke, pickDirectory } from "$lib/platform/api";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let id = $state("my-content");
	let name = $state("My content");
	let version = $state("0.1.0");
	let author = $state("");
	let bot = $state("");
	let devices = $state("");
	let allLanguages = $state(false);
	let busy = $state(false);
	let result = $state<{
		dir: string;
		files: string[];
		classes: string[];
		package: string;
	} | null>(null);

	$effect(() => {
		// Prefill from the selected row: a bot, or a device.
		if (!open) return;
		const p = assembly.page;
		const cells =
			p && assembly.selected !== null
				? p.rows.find(([i]) => i === assembly.selected)?.[1]
				: null;
		if (cells?.BOT_ID !== undefined && p?.table === "DATA_SET_BOTS") bot = String(cells.BOT_ID);
		if (cells?.DEVICE_ID !== undefined && p?.table === "DATA_SET_DEVICES") {
			devices = String(cells.DEVICE_ID);
		}
	});

	async function write() {
		const dir = await pickDirectory(
			"Folder of the mod (created if needed)",
			studio.workspace?.dir,
		);
		if (!dir) return;
		busy = true;
		try {
			result = await rpc("mcts.scaffold", {
				dir: `${dir}/${id}`,
				id,
				name,
				version,
				author,
				bot: bot ? Number(bot) : undefined,
				devices: devices
					.split(/[,\s]+/)
					.map(Number)
					.filter((d) => Number.isFinite(d) && d > 0),
				allLanguages,
			});
			toasts.push(
				"success",
				`${result.files.length} files in ${result.dir}`,
				"Mod folder written",
			);
		} catch (error) {
			toasts.error(error, "Could not write the mod folder");
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title="Export mod folder" width="max-w-xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			Writes a launcher mod: <code>manifest.toml</code>, the assembly and the changed language
			files under <code>files/ChaosGame/…</code> (installed over the game's), the rows the
			private server needs for a champion (<code>paserver.json</code>), and a scaffold for the
			devices' behaviour: UnrealScript class stubs with <code>dllimport</code> functions and a Lua
			script implementing them. The in-memory edits are written as they are; save first if you also
			want them in the game folder.
		</p>
		<div class="grid grid-cols-2 gap-2">
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Mod id</span>
				<input class="input input-sm font-mono" bind:value={id} />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Name</span>
				<input class="input input-sm" bind:value={name} />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Version</span>
				<input class="input input-sm font-mono" bind:value={version} />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Author</span>
				<input class="input input-sm" bind:value={author} />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Champion bot id (for paserver.json)</span>
				<input class="input input-sm font-mono" bind:value={bot} placeholder="e.g. 2599" />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">Device ids to scaffold (comma separated)</span>
				<input
					class="input input-sm font-mono"
					bind:value={devices}
					placeholder="e.g. 33794, 33795"
				/>
			</label>
		</div>
		<label class="flex items-center gap-2 text-xs">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={allLanguages} />
			Include every language file, not only the changed ones
		</label>
		{#if result}
			<div class="bg-base-200 rounded p-2 font-mono text-[11px]">
				<div class="mb-1 flex items-center gap-2">
					<span class="truncate">{result.dir}</span>
					<button
						class="btn btn-ghost btn-xs ml-auto"
						onclick={() => invoke("shell:show-item", { path: result?.dir ?? "" })}
						>Show</button
					>
				</div>
				{#each result.files as f (f)}<div class="opacity-70">{f}</div>{/each}
				{#if result.classes.length > 0}<div class="mt-1">
						classes: {result.classes.join(", ")} ({result.package})
					</div>{/if}
			</div>
		{/if}
		<div class="flex justify-end gap-2">
			<button class="btn btn-sm" onclick={() => (open = false)}>Close</button>
			<button class="btn btn-sm btn-primary" onclick={write} disabled={busy || !id}>
				{#if busy}<span class="loading loading-xs loading-spinner"></span>{/if}
				Write…
			</button>
		</div>
	</div>
</Modal>
