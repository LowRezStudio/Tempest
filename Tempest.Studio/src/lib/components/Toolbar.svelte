<script lang="ts">
	import {
		AppWindow,
		Archive,
		Code2,
		Database,
		FolderOutput,
		Plug,
		RefreshCw,
		Wrench,
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
	/** The package tools: buttons on wide windows, a menu on narrow ones. */
	const tools = $derived([
		{
			label: "Search",
			icon: Search,
			title: "Search every package (Ctrl+K)",
			disabled: false,
			run: onsearch,
		},
		{
			label: "Replace",
			icon: Replace,
			title: "Find and replace in the open package's values and names (Ctrl+H)",
			disabled: !studio.pkg,
			run: () => (replaceOpen = true),
		},
		{
			label: "Batch import",
			icon: FolderInput,
			title: "Import a folder of <package>.<object>.<ext> files",
			disabled: false,
			run: () => (batchOpen = true),
		},
		{
			label: "Wwise audio",
			icon: Music,
			title: "Browse, play, edit and replace the Wwise sounds of the .pck packages",
			disabled: !studio.workspace,
			run: () => (wwiseOpen = true),
		},
		{
			label: "Verify",
			icon: ShieldCheck,
			title: "Round-trip every export of the open package",
			disabled: !studio.pkg,
			run: () => (verifyOpen = true),
		},
		{
			label: "Rewrite",
			icon: Archive,
			title: "Write a compressed or uncompressed copy",
			disabled: !studio.pkg,
			run: () => (rewriteOpen = true),
		},
		{
			label: "Mod folder",
			icon: Package,
			title: "Prepare a mod folder",
			disabled: false,
			run: () => (modOpen = true),
		},
	]);
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
	<Package size={18} class="text-primary mx-1 max-xl:hidden" />
	<button
		class="btn btn-ghost btn-sm max-w-44 font-mono text-xs"
		onclick={switchWorkspace}
		title={studio.workspace?.dir}
	>
		<FolderOpen size={14} class="shrink-0" />
		<span class="truncate">{basename(studio.workspace?.dir ?? "")}</span>
	</button>
	<button
		class="btn btn-ghost btn-sm"
		onclick={openExternal}
		title="Open a package from any folder"
	>
		<FileUp size={14} /><span class="max-2xl:hidden">Open file</span>
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
			<Package size={14} /><span class="max-xl:hidden">Packages</span>
		</button>
		<button
			class="btn join-item btn-sm {inAssembly ? 'btn-active' : 'btn-ghost'}"
			onclick={() => (studio.mode = "assembly")}
			title="assembly.dat (items, devices, skins, bots, effects…) and the Lang_*.dat messages"
		>
			<Database size={14} /><span class="max-xl:hidden">Assembly</span>
		</button>
	</div>
	{#if inAssembly}
		<div class="divider divider-horizontal mx-0"></div>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => assembly.open(true)}
			disabled={assembly.loading}
			title="Reload assembly.dat and the language files from disk"
		>
			<RefreshCw size={14} /><span class="max-2xl:hidden">Reload</span>
		</button>
		<button
			class="btn btn-ghost btn-sm"
			onclick={() => (modExportOpen = true)}
			disabled={!assembly.summary}
			title="Write a mod folder with the changed .dat files, the private server rows and a Lua/UnrealScript scaffold"
		>
			<Package size={14} /><span class="max-2xl:hidden">Export mod…</span>
		</button>
		{#if assembly.summary}
			<span
				class="font-mono text-[11px] opacity-50 max-2xl:hidden"
				title="Field and function tables"
			>
				tokens: {assembly.summary.tokens.split(/[/\\]/).at(-1)}
			</span>
		{/if}
	{:else}
		<div class="divider divider-horizontal mx-0"></div>
		<!-- Wide: every tool as a button (labels from 2xl up). Narrow: a Tools menu. -->
		{#each tools as t (t.label)}
			<button
				class="btn btn-ghost btn-sm max-xl:hidden"
				onclick={t.run}
				disabled={t.disabled}
				title={t.title}
			>
				<t.icon size={14} /><span class="max-2xl:hidden">{t.label}</span>
			</button>
		{/each}
		<div class="dropdown xl:hidden">
			<div
				tabindex="0"
				role="button"
				class="btn btn-ghost btn-sm"
				title="Search, replace, batch import, Wwise audio, verify, rewrite, mod folder"
			>
				<Wrench size={14} /> Tools
			</div>
			<ul class="dropdown-content menu bg-base-200 rounded-box z-20 w-56 p-1 shadow">
				{#each tools as t (t.label)}
					<li>
						<button onclick={t.run} disabled={t.disabled} title={t.title}>
							<t.icon size={14} />
							{t.label}
						</button>
					</li>
				{/each}
			</ul>
		</div>
	{/if}
	<div class="flex-1"></div>
	<button
		class="btn btn-ghost btn-sm"
		onclick={() => (inAssembly ? assembly.undo() : studio.undo())}
		disabled={!canUndo}
		title={canUndo ? `Undo: ${undoLabel} (Ctrl+Z)` : "Nothing to undo (Ctrl+Z)"}
	>
		<Undo2 size={14} /><span class="max-2xl:hidden">Undo</span>
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
		{#if confirmDiscard}Click again to discard{:else}<span class="max-2xl:hidden">Discard</span
			>{/if}
	</button>
	{#if dirtyCount > 0}
		<span class="badge badge-sm badge-warning" title="{dirtyCount} unsaved"
			>{dirtyCount}<span class="max-2xl:hidden">&nbsp;unsaved</span></span
		>
	{/if}
	<button
		class="btn btn-ghost btn-sm"
		onclick={() => (inAssembly ? saveAssemblyAs() : saveAs())}
		disabled={inAssembly ? !assembly.summary : !studio.pkg}
		title={inAssembly ? "Save the changed .dat files into another folder" : "Save a copy"}
	>
		<FolderOutput size={14} /><span class="max-2xl:hidden">Save as…</span>
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
