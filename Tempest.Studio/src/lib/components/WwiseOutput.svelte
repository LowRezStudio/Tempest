<script lang="ts">
	import { pickDirectory } from "$lib/platform/api";

	/** Where replaced .pck files are written: in place, or into a folder (ready for a mod). */
	let { outDir = $bindable<string | null>(null) }: { outDir?: string | null } = $props();

	// One radio group; "folder" without a folder yet opens the picker and
	// falls back to "in place" when it is cancelled.
	let mode = $state<"inplace" | "folder">(outDir ? "folder" : "inplace");

	async function browse() {
		const d = await pickDirectory("Folder for the modified .pck files", outDir ?? undefined);
		if (d) outDir = d;
		mode = outDir ? "folder" : "inplace";
	}

	$effect(() => {
		if (mode === "inplace") {
			outDir = null;
		} else if (!outDir) {
			void browse();
		}
	});
</script>

<div class="flex flex-wrap items-center gap-3 text-xs">
	<span class="opacity-70">Write replaced packages</span>
	<label class="flex items-center gap-1">
		<input
			type="radio"
			class="radio radio-xs"
			name="wwise-out"
			value="inplace"
			bind:group={mode}
		/>
		in place
	</label>
	<label class="flex items-center gap-1">
		<input
			type="radio"
			class="radio radio-xs"
			name="wwise-out"
			value="folder"
			bind:group={mode}
		/>
		into a folder
	</label>
	{#if outDir}
		<span class="font-mono opacity-70" title={outDir}>{outDir}</span>
		<button class="btn btn-xs" onclick={browse}>Browse…</button>
	{/if}
</div>
