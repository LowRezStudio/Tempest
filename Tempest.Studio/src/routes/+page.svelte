<script lang="ts">
	import ExportTree from "$lib/components/ExportTree.svelte";
	import FindReplaceDialog from "$lib/components/FindReplaceDialog.svelte";
	import Inspector from "$lib/components/Inspector.svelte";
	import PackageList from "$lib/components/PackageList.svelte";
	import SearchDialog from "$lib/components/SearchDialog.svelte";
	import StatusBar from "$lib/components/StatusBar.svelte";
	import Toolbar from "$lib/components/Toolbar.svelte";
	import Welcome from "$lib/components/Welcome.svelte";
	import { studio } from "$lib/studio/state.svelte";

	let searchOpen = $state(false);
	let replaceOpen = $state(false);

	function onKeydown(event: KeyboardEvent) {
		const mod = event.ctrlKey || event.metaKey;
		if (!mod) return;
		const k = event.key.toLowerCase();
		if (k === "s" && studio.pkg) {
			event.preventDefault();
			void studio.save();
		} else if ((k === "k" || k === "f") && studio.workspace && !event.shiftKey) {
			event.preventDefault();
			searchOpen = true;
		} else if (k === "z" && studio.pkg && !event.shiftKey) {
			// Inputs and the code editor keep their own undo.
			const t = event.target as HTMLElement | null;
			if (t?.closest("input, textarea, .cm-editor")) return;
			event.preventDefault();
			void studio.undo();
		} else if (k === "h" && studio.pkg) {
			event.preventDefault();
			replaceOpen = true;
		}
	}
</script>

<svelte:window onkeydown={onKeydown} />

{#if !studio.workspace}
	<Welcome />
{:else}
	<div class="flex h-full flex-col">
		<Toolbar onsearch={() => (searchOpen = true)} />
		<div class="flex min-h-0 flex-1">
			<div class="border-base-300 flex w-64 shrink-0 flex-col border-r">
				<PackageList />
			</div>
			<div class="border-base-300 flex w-[26rem] shrink-0 flex-col border-r">
				<ExportTree />
			</div>
			<div class="flex min-w-0 flex-1 flex-col">
				<Inspector />
			</div>
		</div>
		<StatusBar />
	</div>
	<SearchDialog bind:open={searchOpen} />
	<FindReplaceDialog bind:open={replaceOpen} />
{/if}
