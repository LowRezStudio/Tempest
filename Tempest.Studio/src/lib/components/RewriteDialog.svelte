<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickSave, PACKAGE_FILTERS } from "$lib/platform/api";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let compress = $state(true);
	let obscure = $state(true);

	$effect(() => {
		if (open && studio.pkg) {
			compress = true;
			obscure = studio.pkg.summary.obscured || studio.pkg.summary.compressed;
		}
	});

	async function go() {
		if (!studio.pkg) return;
		const path = studio.pkg.path;
		const out = await pickSave(
			"Write rewritten package",
			path.replace(/\.upk$/i, compress ? ".compressed.upk" : ".decompressed.upk"),
			PACKAGE_FILTERS,
		);
		if (!out) return;
		try {
			const r = await studio.run("Rewriting…", () =>
				rpc("package.rewrite", {
					path,
					out,
					compress,
					obscure: compress ? obscure : false,
				}),
			);
			toasts.push("success", r.out, compress ? "Compressed" : "Decompressed");
			await studio.refreshPackages();
			open = false;
		} catch (error) {
			toasts.error(error, "Rewrite failed");
		}
	}
</script>

<Modal bind:open title="Rewrite {studio.pkg?.name ?? ''}" width="max-w-md">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-60">
			Writes a copy of the file on disk (unsaved edits are not included) in the chosen
			storage. The game loads both forms; the cooker ships compressed and obscured packages.
		</p>
		<label class="flex items-center gap-2"
			><input type="checkbox" class="checkbox checkbox-sm" bind:checked={compress} /> LZO-compress
			in cooker-style chunks</label
		>
		<label class="flex items-center gap-2"
			><input
				type="checkbox"
				class="checkbox checkbox-sm"
				bind:checked={obscure}
				disabled={!compress}
			/> XOR-obscure the chunks (Hi-Rez flag)</label
		>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" onclick={() => (open = false)}>Cancel</button>
			<button class="btn btn-sm btn-primary" onclick={go}>Write…</button>
		</div>
	</div>
</Modal>
