<script lang="ts">
	import {
		Archive,
		Code2,
		FolderInput,
		Replace,
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
	import ModDialog from "$lib/components/ModDialog.svelte";
	import RewriteDialog from "$lib/components/RewriteDialog.svelte";
	import VerifyDialog from "$lib/components/VerifyDialog.svelte";
	import { pickDirectory, pickSave, PACKAGE_FILTERS } from "$lib/platform/api";
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
	let replaceOpen = $state(false);
	let batchOpen = $state(false);
	let verifyOpen = $state(false);
	let rewriteOpen = $state(false);

	async function switchWorkspace() {
		const dir = await pickDirectory("Open a cooked folder", studio.workspace?.dir);
		if (dir) await studio.openWorkspace(dir);
	}

	const isScriptPackage = $derived(
		studio.pkg ? (studio.pkg.summary.packageFlags & CONTAINS_SCRIPT) !== 0 : false,
	);

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
	<div class="divider divider-horizontal mx-0"></div>
	<button class="btn btn-ghost btn-sm" onclick={onsearch} title="Search every package (Ctrl+K)">
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
	<div class="flex-1"></div>
	{#if studio.dirty.length > 0}
		<span class="badge badge-sm badge-warning">{studio.dirty.length} unsaved</span>
	{/if}
	<button
		class="btn btn-ghost btn-sm"
		onclick={saveAs}
		disabled={!studio.pkg}
		title="Save a copy"
	>
		Save as…
	</button>
	<button
		class="btn btn-sm btn-primary"
		onclick={() => studio.save()}
		disabled={!studio.isDirty}
		title="Save in place (Ctrl+S)"
	>
		<Save size={14} /> Save
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
<RewriteDialog bind:open={rewriteOpen} />
