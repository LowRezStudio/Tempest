<script lang="ts">
	import AssemblyView from "$lib/components/AssemblyView.svelte";
	import ExportTree from "$lib/components/ExportTree.svelte";
	import FindReplaceDialog from "$lib/components/FindReplaceDialog.svelte";
	import Inspector from "$lib/components/Inspector.svelte";
	import PackageList from "$lib/components/PackageList.svelte";
	import SearchDialog from "$lib/components/SearchDialog.svelte";
	import StatusBar from "$lib/components/StatusBar.svelte";
	import Toolbar from "$lib/components/Toolbar.svelte";
	import Welcome from "$lib/components/Welcome.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { studio } from "$lib/studio/state.svelte";

	let searchOpen = $state(false);
	let replaceOpen = $state(false);

	function onKeydown(event: KeyboardEvent) {
		const inAssembly = studio.mode === "assembly";
		if (event.altKey && !event.ctrlKey && !event.metaKey) {
			if (event.key === "ArrowLeft") {
				event.preventDefault();
				void (inAssembly ? assembly.back() : studio.back());
			} else if (event.key === "ArrowRight") {
				event.preventDefault();
				void (inAssembly ? assembly.forward() : studio.forward());
			}
			return;
		}
		const mod = event.ctrlKey || event.metaKey;
		if (!mod) return;
		const k = event.key.toLowerCase();
		if (k === "n" && event.shiftKey) {
			event.preventDefault();
			void studio.newWindow();
		} else if (inAssembly) {
			const t = event.target as HTMLElement | null;
			if (k === "s") {
				event.preventDefault();
				void assembly.saveAll();
			} else if (k === "z" && !event.shiftKey && !t?.closest("input, textarea")) {
				event.preventDefault();
				void assembly.undo();
			}
		} else if (k === "s" && studio.pkg) {
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
	// Mouse side buttons (3 = back, 4 = forward).
	function onMouseUp(event: MouseEvent) {
		const nav = studio.mode === "assembly" ? assembly : studio;
		if (event.button === 3) void nav.back();
		else if (event.button === 4) void nav.forward();
	}
</script>

<svelte:window onkeydown={onKeydown} onmouseup={onMouseUp} />

{#if !studio.workspace}
	<Welcome />
{:else}
	<div class="flex h-full flex-col">
		<Toolbar onsearch={() => (searchOpen = true)} />
		{#if studio.mode === "assembly"}
			<AssemblyView />
		{:else}
			<div class="flex min-h-0 flex-1">
				<div class="border-base-300 flex w-52 shrink-0 flex-col border-r xl:w-64">
					<PackageList />
				</div>
				<div class="border-base-300 flex w-80 shrink-0 flex-col border-r xl:w-[26rem]">
					<ExportTree />
				</div>
				<div class="flex min-w-0 flex-1 flex-col">
					<Inspector />
				</div>
			</div>
		{/if}
		<StatusBar />
	</div>
	<SearchDialog bind:open={searchOpen} />
	<FindReplaceDialog bind:open={replaceOpen} />
{/if}
