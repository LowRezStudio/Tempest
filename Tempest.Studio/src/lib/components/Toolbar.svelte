<script lang="ts">
	import {
		AppWindow,
		Archive,
		Code2,
		Database,
		Plug,
		RefreshCw,
		FileUp,
		FolderInput,
		Music,
		Replace,
		RotateCcw,
		Undo2,
		FolderOpen,
		Moon,
		Package,
		Save,
		Search,
		ShieldCheck,
		Sun,
	} from "@lucide/svelte";
	import BatchImportDialog from "$lib/components/BatchImportDialog.svelte";
	import FindReplaceDialog from "$lib/components/FindReplaceDialog.svelte";
	import McpDialog from "$lib/components/McpDialog.svelte";
	import ModDialog from "$lib/components/ModDialog.svelte";
	import ModExportDialog from "$lib/components/ModExportDialog.svelte";
	import RewriteDialog from "$lib/components/RewriteDialog.svelte";
	import VerifyDialog from "$lib/components/VerifyDialog.svelte";
	import WwiseDialog from "$lib/components/WwiseDialog.svelte";
	import { pickDirectory, pickFile, pickSave, PACKAGE_FILTERS } from "$lib/platform/api";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { CONTAINS_SCRIPT } from "$lib/studio/classes";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	interface Props {
		onsearch: () => void;
	}
	const { onsearch }: Props = $props();

	let modOpen = $state(false);
	let confirmDiscard = $state(false);
	let replaceOpen = $state(false);
	let batchOpen = $state(false);
	let verifyOpen = $state(false);
	let rewriteOpen = $state(false);
	let wwiseOpen = $state(false);
	let mcpOpen = $state(false);
	let modExportOpen = $state(false);

	async function switchWorkspace() {
		const dir = await pickDirectory("Open a cooked folder", studio.workspace?.dir);
		if (dir) await studio.openWorkspace(dir);
	}

	const isScriptPackage = $derived(
		studio.pkg ? (studio.pkg.summary.packageFlags & CONTAINS_SCRIPT) !== 0 : false,
	);

	async function openExternal() {
		const f = await pickFile("Open a package from anywhere", PACKAGE_FILTERS);
		if (f) await studio.openFiles([f]);
	}

	async function saveAssemblyAs() {
		const dir = await pickDirectory("Folder for the saved .dat files", studio.workspace?.dir);
		if (dir) await assembly.saveAll(dir);
	}

	const inAssembly = $derived(studio.mode === "assembly");
	const canUndo = $derived(
		inAssembly ? assembly.undoLabels.length > 0 : studio.undoLabels.length > 0,
	);
	const undoLabel = $derived(inAssembly ? assembly.undoLabels[0] : studio.undoLabels[0]);
	const dirtyCount = $derived(inAssembly ? assembly.dirtyDocs.length : studio.dirty.length);
	const canSave = $derived(inAssembly ? assembly.dirtyDocs.length > 0 : studio.isDirty);

	async function exportSources() {
		if (!studio.pkg) return;
		const path = studio.pkg.path;
		const outDir = await pickDirectory(
			"Folder for <Package>/Classes/*.uc",
			studio.workspace?.dir,
		);
		if (!outDir) return;
		try {
			const r = await studio.run("Decompiling…", () =>
				rpc("script.export", { path, outDir }),
			);
			toasts.push("success", `${r.classes} classes in ${r.dir}`, "Sources exported");
		} catch (error) {
			toasts.error(error, "Decompile failed");
		}
	}

	async function saveAs() {
		if (!studio.pkg) return;
		const out = await pickSave("Save package as", studio.pkg.path, PACKAGE_FILTERS, "package");
		if (out) await studio.save(out);
	}
</script>

<div class="border-base-300 bg-base-200 flex h-11 shrink-0 items-center gap-1 border-b px-2">
	<Package size={18} class="text-primary mx-1" />
	<button
		class="btn btn-ghost btn-sm font-mono text-xs"
		onclick={switchWorkspace}
		title={studio.workspace?.dir}
	>
		<FolderOpen size={14} />
		{basename(studio.workspace?.dir ?? "")}
	</button>
	<button
		class="btn btn-ghost btn-sm"
		onclick={openExternal}
		title="Open a package from any folder"
	>
		<FileUp size={14} /> Open file
	</button>
	<button
		class="btn btn-ghost btn-sm"
		onclick={() => studio.newWindow()}
		title="Open another window to work on a second workspace (Ctrl+Shift+N)"
	>
		<AppWindow size={14} />
	</button>
	<div class="join mx-1">
		<button
			class="btn join-item btn-sm {inAssembly ? 'btn-ghost' : 'btn-active'}"
			onclick={() => (studio.mode = "packages")}
			title="Packages: textures, meshes, sounds, scripts…"
		>
			<Package size={14} /> Packages
		</button>
		<button
			class="btn join-item btn-sm {inAssembly ? 'btn-active' : 'btn-ghost'}"
			onclick={() => (studio.mode = "assembly")}
			title="assembly.dat (items, devices, skins, bots, effects…) and the Lang_*.dat messages"
		>
			<Database size={14} /> Assembly
		</button>
	</div>
	{#if inAssembly}
		<div class="divider divider-horizontal mx-0"></div>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => assembly.open()}
			disabled={assembly.loading}
			title="Reload assembly.dat and the language files from disk"
		>
			<RefreshCw size={14} /> Reload
		</button>
		{#if assembly.summary}
			<span class="font-mono text-[11px] opacity-50" title="Field and function tables">
				tokens: {assembly.summary.tokens.split(/[/\\]/).at(-1)}
			</span>
		{/if}
	{:else}
		<div class="divider divider-horizontal mx-0"></div>
		<button
			class="btn btn-ghost btn-sm"
			onclick={onsearch}
			title="Search every package (Ctrl+K)"
		>
			<Search size={14} /> Search
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (replaceOpen = true)}
			disabled={!studio.pkg}
			title="Find and replace in the open package's values and names (Ctrl+H)"
		>
			<Replace size={14} /> Replace
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (batchOpen = true)}
			title="Import a folder of <package>.<object>.<ext> files"
		>
			<FolderInput size={14} /> Batch import
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (wwiseOpen = true)}
			disabled={!studio.workspace}
			title="Browse, play, edit and replace the Wwise sounds of the .pck packages"
		>
			<Music size={14} /> Wwise audio
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (verifyOpen = true)}
			disabled={!studio.pkg}
			title="Round-trip every export of the open package"
		>
			<ShieldCheck size={14} /> Verify
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (rewriteOpen = true)}
			disabled={!studio.pkg}
			title="Write a compressed or uncompressed copy"
		>
			<Archive size={14} /> Rewrite
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (modOpen = true)}
			title="Prepare a mod folder"
		>
			<Package size={14} /> Mod folder
		</button>
	{/if}
	<div class="flex-1"></div>
	<button
		class="btn btn-ghost btn-sm"
		onclick={() => (inAssembly ? assembly.undo() : studio.undo())}
		disabled={!canUndo}
		title={canUndo ? `Undo: ${undoLabel} (Ctrl+Z)` : "Nothing to undo (Ctrl+Z)"}
	>
		<Undo2 size={14} /> Undo
	</button>
	<button
		class="btn btn-sm {confirmDiscard ? 'btn-error' : 'btn-ghost'}"
		onclick={() => {
			if (confirmDiscard) {
				confirmDiscard = false;
				if (inAssembly) {
					if (assembly.doc) void assembly.discard(assembly.doc.id);
				} else {
					void studio.discard();
				}
			} else {
				confirmDiscard = true;
			}
		}}
		onblur={() => (confirmDiscard = false)}
		disabled={inAssembly ? !assembly.isDirty : !studio.isDirty}
		title={inAssembly
			? "Throw away every unsaved change of this file and reload it from disk"
			: "Throw away every unsaved change of this package and reload it from disk"}
	>
		<RotateCcw size={14} />
		{confirmDiscard ? "Click again to discard" : "Discard"}
	</button>
	{#if dirtyCount > 0}
		<span class="badge badge-sm badge-warning">{dirtyCount} unsaved</span>
	{/if}
	<button
		class="btn btn-ghost btn-sm"
		onclick={() => (inAssembly ? saveAssemblyAs() : saveAs())}
		disabled={inAssembly ? !assembly.summary : !studio.pkg}
		title={inAssembly ? "Save the changed .dat files into another folder" : "Save a copy"}
	>
		Save as…
	</button>
	<button
		class="btn btn-sm btn-primary"
		onclick={() => (inAssembly ? assembly.saveAll() : studio.save())}
		disabled={!canSave}
		title={inAssembly
			? "Save every changed .dat file in place (Ctrl+S)"
			: "Save in place (Ctrl+S)"}
	>
		<Save size={14} /> Save
	</button>
	<button
		class="btn btn-square btn-ghost btn-sm"
		onclick={() => (mcpOpen = true)}
		title="MCP server: let agents drive this Studio"
	>
		<Plug size={14} />
	</button>
	<button
		class="btn btn-square btn-ghost btn-sm"
		onclick={() => (studio.theme = studio.theme === "mocha" ? "latte" : "mocha")}
		title="Toggle theme"
	>
		{#if studio.theme === "mocha"}<Sun size={14} />{:else}<Moon size={14} />{/if}
	</button>
</div>

<ModDialog bind:open={modOpen} />
<FindReplaceDialog bind:open={replaceOpen} />
<BatchImportDialog bind:open={batchOpen} />
<VerifyDialog bind:open={verifyOpen} />
<WwiseDialog bind:open={wwiseOpen} />
<McpDialog bind:open={mcpOpen} />
<ModExportDialog bind:open={modExportOpen} />
<RewriteDialog bind:open={rewriteOpen} />
