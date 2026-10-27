<script lang="ts">
	import { History, X } from "@lucide/svelte";
	import { onMount } from "svelte";
	import AssemblyView from "$lib/components/AssemblyView.svelte";
	import BatchImportDialog from "$lib/components/BatchImportDialog.svelte";
	import ChampionVendorDialog from "$lib/components/ChampionVendorDialog.svelte";
	import FindReplaceDialog from "$lib/components/FindReplaceDialog.svelte";
	import McpDialog from "$lib/components/McpDialog.svelte";
	import ModDialog from "$lib/components/ModDialog.svelte";
	import ModExportDialog from "$lib/components/ModExportDialog.svelte";
	import ModView from "$lib/components/ModView.svelte";
	import NewContentDialog from "$lib/components/NewContentDialog.svelte";
	import PackagesView from "$lib/components/PackagesView.svelte";
	import RewriteDialog from "$lib/components/RewriteDialog.svelte";
	import SearchDialog from "$lib/components/SearchDialog.svelte";
	import ActivityBar from "$lib/components/shell/ActivityBar.svelte";
	import AppHeader from "$lib/components/shell/AppHeader.svelte";
	import CommandPalette from "$lib/components/shell/CommandPalette.svelte";
	import ConfirmDialog from "$lib/components/shell/ConfirmDialog.svelte";
	import NewProjectDialog from "$lib/components/shell/NewProjectDialog.svelte";
	import SaveWorkspaceDialog from "$lib/components/shell/SaveWorkspaceDialog.svelte";
	import ShortcutsDialog from "$lib/components/shell/ShortcutsDialog.svelte";
	import TabBar from "$lib/components/shell/TabBar.svelte";
	import StatusBar from "$lib/components/StatusBar.svelte";
	import VerifyDialog from "$lib/components/VerifyDialog.svelte";
	import Welcome from "$lib/components/Welcome.svelte";
	import WwiseDialog from "$lib/components/WwiseDialog.svelte";
	import { registerActions } from "$lib/studio/actions";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { commands } from "$lib/studio/commands.svelte";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { nav } from "$lib/studio/nav.svelte";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { ui, type DialogId } from "$lib/studio/ui.svelte";

	onMount(() => registerActions());

	function errorText(error: unknown): string {
		return error instanceof Error ? error.message : String(error);
	}
	function reportError(error: unknown): void {
		console.error(error);
	}

	const isOpen = (id: DialogId) => ui.dialog === id;
	function setOpen(id: DialogId, v: boolean) {
		if (v) ui.dialog = id;
		else if (ui.dialog === id) ui.dialog = null;
	}

	// Pane layout travels with the workspace file.
	session.extraState = () => ({
		cooker: cooker.snapshot(),
		panes: $state.snapshot(ui.panes),
		collapsed: $state.snapshot(ui.collapsed),
		tabs: $state.snapshot(tabs.list),
		activeTab: tabs.active,
	});
	session.extraRestore = (s) => {
		const v = s as {
			panes?: Record<string, number>;
			collapsed?: Record<string, boolean>;
			tabs?: typeof tabs.list;
			cooker?: unknown;
			activeTab?: number | null;
		} | null;
		if (!v) return;
		if (v.cooker) cooker.restore(v.cooker);
		if (v.panes) ui.panes = v.panes;
		if (v.collapsed) ui.collapsed = v.collapsed;
		if (v.tabs?.length) {
			tabs.list = v.tabs;
			tabs.active = v.activeTab ?? v.tabs[0]?.id ?? null;
		}
	};

	// A new game folder starts with fresh tabs.
	let lastDir: string | null = null;
	$effect(() => {
		const dir = studio.workspace?.dir ?? null;
		if (dir !== lastDir) {
			lastDir = dir;
			tabs.clear();
		}
	});

	// Becoming narrow folds the side bar away; becoming wide brings it back.
	let wasCompact = ui.compact;
	$effect(() => {
		const c = ui.compact;
		if (c !== wasCompact) {
			wasCompact = c;
			ui.toggle("side", c);
		}
	});
	if (ui.compact) ui.toggle("side", true);

	function onKeydown(event: KeyboardEvent) {
		if (ui.palette) return;
		commands.handle(event);
	}

	// Mouse side buttons (3 = back, 4 = forward).
	function onMouseUp(event: MouseEvent) {
		if (event.button === 3) void nav.back();
		else if (event.button === 4) void nav.forward();
	}

	const newKind = $derived(
		assembly.page?.table === "DATA_SET_BOTS"
			? ("champion" as const)
			: assembly.page?.table === "DATA_SET_ITEMS"
				? ("item" as const)
				: ("device" as const),
	);
</script>

<svelte:window onkeydown={onKeydown} onmouseup={onMouseUp} />

{#if !studio.workspace}
	<Welcome />
{:else}
	<div class="flex h-full flex-col">
		<AppHeader />
		{#if session.recovery}
			<div
				class="bg-warning/15 border-warning/40 flex items-center gap-2 border-b px-3 py-1 text-xs"
			>
				<History size={13} />
				<span class="flex-1"
					>Unsaved work from an earlier session of this game folder was kept ({new Date(
						session.recovery.mtime,
					).toLocaleString()}).</span
				>
				<button class="btn btn-warning btn-xs" onclick={() => session.restoreRecovery()}
					>Restore it</button
				>
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => session.clearRecovery()}
					title="Delete the recovery copy"
				>
					<X size={12} /> Dismiss
				</button>
			</div>
		{/if}
		<div class="flex min-h-0 flex-1">
			<ActivityBar />
			<main class="flex min-w-0 flex-1 flex-col">
				<TabBar />
				<svelte:boundary onerror={reportError}>
					{#if studio.mode === "assembly"}
						<AssemblyView />
					{:else if studio.mode === "mod"}
						<ModView />
					{:else}
						<PackagesView />
					{/if}
					{#snippet failed(error, reset)}
						{@render crashed(error, reset)}
					{/snippet}
				</svelte:boundary>
			</main>
		</div>
		<StatusBar />
	</div>
	<svelte:boundary onerror={reportError}>
		<SearchDialog bind:open={() => isOpen("search"), (v) => setOpen("search", v)} />
		<FindReplaceDialog bind:open={() => isOpen("replace"), (v) => setOpen("replace", v)} />
		<BatchImportDialog bind:open={() => isOpen("batch"), (v) => setOpen("batch", v)} />
		<VerifyDialog bind:open={() => isOpen("verify"), (v) => setOpen("verify", v)} />
		<RewriteDialog bind:open={() => isOpen("rewrite"), (v) => setOpen("rewrite", v)} />
		<ModDialog bind:open={() => isOpen("modFolder"), (v) => setOpen("modFolder", v)} />
		<WwiseDialog bind:open={() => isOpen("wwise"), (v) => setOpen("wwise", v)} />
		<ModExportDialog bind:open={() => isOpen("modExport"), (v) => setOpen("modExport", v)} />
		<ChampionVendorDialog bind:open={() => isOpen("vendor"), (v) => setOpen("vendor", v)} />
		<SaveWorkspaceDialog
			bind:open={() => isOpen("saveWorkspace"), (v) => setOpen("saveWorkspace", v)}
		/>
		<NewContentDialog
			bind:open={() => isOpen("newContent"), (v) => setOpen("newContent", v)}
			kind={newKind}
		/>
		{#snippet failed(error, reset)}
			{@render dialogCrashed(error, reset)}
		{/snippet}
	</svelte:boundary>
{/if}

<!-- A view that throws while rendering shows this instead of an empty window. -->
{#snippet crashed(error: unknown, reset: () => void)}
	<div class="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
		<p class="font-semibold">This view stopped working.</p>
		<p class="max-w-xl font-mono text-xs [overflow-wrap:anywhere] opacity-70">
			{errorText(error)}
		</p>
		<p class="text-xs opacity-60">Your edits are not lost.</p>
		<button class="btn btn-primary btn-sm" onclick={reset}>Show it again</button>
	</div>
{/snippet}

{#snippet dialogCrashed(error: unknown, reset: () => void)}
	<div class="modal modal-open" role="alertdialog" aria-label="Dialog error">
		<div class="modal-box flex flex-col gap-3">
			<p class="font-semibold">This dialog stopped working.</p>
			<p class="font-mono text-xs [overflow-wrap:anywhere] opacity-70">{errorText(error)}</p>
			<div class="flex justify-end">
				<button
					class="btn btn-primary btn-sm"
					onclick={() => {
						ui.dialog = null;
						reset();
					}}>Close</button
				>
			</div>
		</div>
	</div>
{/snippet}
<McpDialog bind:open={() => isOpen("mcp"), (v) => setOpen("mcp", v)} />
<ShortcutsDialog bind:open={() => isOpen("shortcuts"), (v) => setOpen("shortcuts", v)} />
<NewProjectDialog bind:open={() => isOpen("newProject"), (v) => setOpen("newProject", v)} />
<CommandPalette />
<ConfirmDialog />
