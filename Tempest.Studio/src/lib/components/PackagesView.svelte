<!-- Packages | export tree | inspector, the first two resizable and
	collapsible (Ctrl+B, Ctrl+J); on narrow windows the package list floats. -->
<script lang="ts">
	import ExportTree from "$lib/components/ExportTree.svelte";
	import Inspector from "$lib/components/Inspector.svelte";
	import PackageList from "$lib/components/PackageList.svelte";
	import Pane from "$lib/components/shell/Pane.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	// A package picked from the floating list: give the room back to the editor.
	let lastPkg: string | null = null;
	$effect(() => {
		const path = studio.pkg?.path ?? null;
		if (path !== lastPkg) {
			lastPkg = path;
			if (ui.compact && path) ui.toggle("side", true);
		}
	});
</script>

<div class="relative flex min-h-0 flex-1">
	{#if !ui.isCollapsed("side")}
		<Pane id="pkg.list" side="left" initial={240} min={170} max={520} overlay={ui.compact}>
			<PackageList />
		</Pane>
	{/if}
	{#if !ui.isCollapsed("details")}
		<Pane id="pkg.tree" side="left" initial={ui.compact ? 300 : 380} min={220} max={900}>
			<ExportTree />
		</Pane>
	{/if}
	<div class="flex min-w-0 flex-1 flex-col">
		<Inspector />
	</div>
</div>
