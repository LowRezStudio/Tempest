<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import type { VerifyReport } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let report = $state<VerifyReport | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open || !studio.pkg) return;
		report = null;
		error = null;
		const path = studio.pkg.path;
		void studio
			.run("Verifying round trip…", () => rpc("package.verify", { path }))
			.then((r) => (report = r))
			.catch((error_: unknown) => {
				error = error_ instanceof Error ? error_.message : String(error_);
			});
	});

	const pct = (n: number, d: number) => (d ? ((n * 100) / d).toFixed(2) : "0");
</script>

<Modal bind:open title="Verify {studio.pkg?.name ?? ''}" width="max-w-2xl">
	{#if error}
		<p class="text-error text-sm">{error}</p>
	{:else if !report}
		<div class="flex items-center gap-2 text-sm">
			<span class="loading loading-sm loading-spinner"></span> Parsing and rewriting every export
			of the file on disk…
		</div>
	{:else}
		<div class="alert {report.ok ? 'alert-success' : 'alert-error'} mb-3 text-sm">
			{report.ok
				? "Every export re-serialises byte-for-byte and the package image rebuilds identically."
				: (report.error ??
					`${report.mismatches} export mismatches${report.imageIdentical ? "" : ", image differs"}`)}
		</div>
		<table class="table-xs table font-mono">
			<tbody>
				<tr><td>exports</td><td>{report.exports}</td></tr>
				<tr
					><td>fully typed</td><td>{report.full} ({pct(report.full, report.exports)}%)</td
					></tr
				>
				<tr><td>with native tail not decoded</td><td>{report.withTail}</td></tr>
				<tr><td>with raw property values</td><td>{report.partialValues}</td></tr>
				<tr><td>header unparsed</td><td>{report.raw}</td></tr>
				<tr><td>round-trip mismatches</td><td>{report.mismatches}</td></tr>
			</tbody>
		</table>
		{#if report.mismatchSamples.length}
			<h4 class="mt-3 text-xs font-semibold">Mismatches</h4>
			<pre class="selectable text-xs">{report.mismatchSamples.join("\n")}</pre>
		{/if}
		{#if report.rawSamples.length}
			<h4 class="mt-3 text-xs font-semibold">Not fully decoded</h4>
			<pre class="selectable text-xs">{report.rawSamples.join("\n")}</pre>
		{/if}
		<p class="mt-3 text-xs opacity-60">
			Verification reads the file on disk, not unsaved edits.
		</p>
	{/if}
</Modal>
