<script lang="ts">
	import {
		ChevronLeft,
		ChevronRight,
		Copy,
		CopyPlus,
		Download,
		FileCode2,
		Link2,
		Pencil,
		Trash2,
		Upload,
	} from "@lucide/svelte";
	import AnimExportDialog from "$lib/components/AnimExportDialog.svelte";
	import AnimPreview from "$lib/components/AnimPreview.svelte";
	import ClassIcon from "$lib/components/ClassIcon.svelte";
	import CopyDialog from "$lib/components/CopyDialog.svelte";
	import DepsPanel from "$lib/components/DepsPanel.svelte";
	import ImportDialog from "$lib/components/ImportDialog.svelte";
	import MaterialPreview from "$lib/components/MaterialPreview.svelte";
	import MeshPreview from "$lib/components/MeshPreview.svelte";
	import ParticlePreview from "$lib/components/ParticlePreview.svelte";
	import PropertyTree from "$lib/components/PropertyTree.svelte";
	import RenameDialog from "$lib/components/RenameDialog.svelte";
	import ScriptEditor from "$lib/components/ScriptEditor.svelte";
	import SoundPreview from "$lib/components/SoundPreview.svelte";
	import SwfPreview from "$lib/components/SwfPreview.svelte";
	import TexturePreview from "$lib/components/TexturePreview.svelte";
	import WwisePreview from "$lib/components/WwisePreview.svelte";
	import { invoke, pickSave } from "$lib/platform/api";
	import { isScriptExport, previewFor } from "$lib/studio/classes";
	import { keyLabel } from "$lib/studio/commands.svelte";
	import { bytes, safeName } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	type Tab = "properties" | "preview" | "native" | "deps" | "text" | "source";
	let tab = $state<Tab>("properties");
	let renameOpen = $state(false);
	let copyOpen = $state(false);
	let importOpen = $state(false);
	let animExportOpen = $state(false);
	let text = $state<string | null>(null);

	const row = $derived(studio.selectedRow);
	const obj = $derived(studio.object);
	const preview = $derived(
		row ? previewFor(row.class, obj?.kind === "object" ? obj.native.type : undefined) : "none",
	);
	const placeholder = $derived(row ? row.size === 0 && row.classRef === 0 : false);
	// Flag names are not in the compact tables: fetched for the selected row.
	let flagsText = $state("");
	$effect(() => {
		const path = studio.pkg?.path;
		const index = row?.i;
		flagsText = row?.flagsHex ?? "";
		if (!path || index === undefined) return;
		void rpc("package.export", { path, index }).then((full) => {
			if (studio.pkg?.path === path && studio.selected === index) flagsText = full.flags;
		});
	});
	const script = $derived(row ? !placeholder && isScriptExport(row) : false);

	// Jump to the preview tab for previewable assets, back otherwise.
	$effect(() => {
		const i = studio.selected;
		if (i === null) return;
		text = null;
		if (preview !== "none") tab = "preview";
		else if (script) tab = "source";
		else if (tab === "preview" || tab === "source") tab = "properties";
	});

	// An action asked from the export tree's context menu, once this export shows.
	$effect(() => {
		const a = ui.objectAction;
		if (!a || !row || row.i !== a.index) return;
		ui.objectAction = null;
		if (placeholder && a.kind !== "raw") return;
		if (a.kind === "export") void exportAsset();
		else if (a.kind === "raw") void extractRaw();
		else if (a.kind === "replace") importOpen = true;
		else if (a.kind === "rename") renameOpen = true;
		else if (a.kind === "copy") copyOpen = true;
		else if (a.kind === "delete") void deleteObject();
	});

	async function loadText() {
		if (!studio.pkg || studio.selected === null) return;
		try {
			text = await rpc("object.text", { path: studio.pkg.path, index: studio.selected });
		} catch (error) {
			toasts.error(error);
		}
	}
	$effect(() => {
		if (tab === "text" && text === null) void loadText();
	});

	async function extractRaw() {
		if (!studio.pkg || !row) return;
		const out = await pickSave("Extract raw payload", `${safeName(row.path)}.bin`, [
			{ name: "Binary", extensions: ["bin"] },
		]);
		if (!out) return;
		try {
			const r = await rpc("object.extract", { path: studio.pkg.path, index: row.i, out });
			toasts.push("success", `${bytes(r.bytes)} written to ${out}`, "Extracted");
		} catch (error) {
			toasts.error(error);
		}
	}

	async function exportAsset() {
		if (!studio.pkg || !row) return;
		const path = studio.pkg.path;
		try {
			if (preview === "texture") {
				const out = await pickSave("Export texture", `${safeName(row.path)}.png`, [
					{ name: "PNG image", extensions: ["png"] },
					{ name: "DDS (original compressed data)", extensions: ["dds"] },
				]);
				if (!out) return;
				const r = await studio.run("Exporting texture…", () =>
					rpc("texture.export", { path, index: row.i, out }),
				);
				toasts.push("success", `${bytes(r.bytes)} → ${out}`, "Exported");
			} else if (preview === "anim") {
				animExportOpen = true;
			} else if (preview === "mesh") {
				const out = await pickSave("Export mesh", `${safeName(row.path)}.glb`, [
					{ name: "glTF binary (textures + skeleton)", extensions: ["glb"] },
					{ name: "Autodesk FBX (textures + skeleton)", extensions: ["fbx"] },
					{ name: "Wavefront OBJ", extensions: ["obj"] },
				]);
				if (!out) return;
				const r = await studio.run("Exporting mesh…", () =>
					rpc("mesh.export", { path, index: row.i, out }),
				);
				for (const w of r.warnings) toasts.push("warning", w, "Texture skipped");
				toasts.push("success", `${bytes(r.bytes)} → ${out}`, "Exported");
			} else if (preview === "sound" || preview === "swf") {
				const ext = preview === "sound" ? "ogg" : "gfx";
				const out = await pickSave("Export", `${safeName(row.path)}.${ext}`, [
					{
						name: preview === "sound" ? "Ogg Vorbis" : "Scaleform movie",
						extensions: [ext],
					},
				]);
				if (!out) return;
				const r = await rpc("media.export", { path, index: row.i, out });
				toasts.push("success", `${bytes(r.bytes)} → ${out}`, "Exported");
			} else {
				await extractRaw();
			}
		} catch (error) {
			toasts.error(error, "Export failed");
		}
	}

	async function deleteObject() {
		if (!studio.pkg || !row) return;
		const ok = await ui.confirm({
			title: `Delete ${row.name}?`,
			message:
				"The object and its sub-objects become empty placeholders and every reference to them is cleared. Undo (Ctrl+Z) brings them back.",
			items: [row.path],
			confirm: "Delete",
			danger: true,
		});
		if (ok !== true) return;
		try {
			const r = await rpc("object.delete", { path: studio.pkg.path, indices: [row.i] });
			studio.markDirty();
			await studio.reloadTables();
			await studio.select(row.i);
			toasts.push("success", `${r.cleared} references cleared`, `Deleted ${row.name}`);
		} catch (error) {
			toasts.error(error, "Delete failed");
		}
	}

	async function showInFolder() {
		if (studio.pkg) await invoke("shell:show-item", { path: studio.pkg.path });
	}
</script>

{#if !row || !studio.pkg}
	<div class="flex h-full items-center justify-center text-sm opacity-50">Select an export</div>
{:else}
	<div class="border-base-300 flex shrink-0 flex-col gap-2 border-b px-3 py-2">
		<div class="flex items-center gap-2">
			<ClassIcon className={row.class} size={18} />
			<div class="min-w-0 flex-1">
				<div class="selectable truncate font-mono text-sm font-semibold" title={row.path}>
					{row.path}
				</div>
				<div class="selectable truncate font-mono text-[11px] opacity-60">
					{row.class} · export #{row.i} · {bytes(row.size)} @ {row.offset} · {flagsText}
				</div>
			</div>
		</div>
		<div class="flex flex-wrap items-center gap-1">
			<button
				class="btn btn-sm"
				onclick={exportAsset}
				disabled={placeholder}
				title="Export this asset to a file"
			>
				<Download size={14} />
				{preview === "none" ? "Extract raw" : "Export"}
			</button>
			<button
				class="btn btn-sm"
				onclick={() => (importOpen = true)}
				disabled={placeholder}
				title={preview === "none"
					? "Replace the export's raw payload from a file"
					: "Replace the asset's data from a file"}
			>
				<Upload size={14} /> Replace…
			</button>
			{#if preview !== "none"}
				<button
					class="btn btn-ghost btn-sm"
					onclick={extractRaw}
					title="Raw serialized payload"
				>
					<FileCode2 size={14} /> Raw
				</button>
			{/if}
			<div class="divider divider-horizontal mx-0"></div>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => (renameOpen = true)}
				disabled={placeholder}
			>
				<Pencil size={14} /> Rename
			</button>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => studio.duplicate()}
				disabled={placeholder}
				title="A copy of this object next to it, named …_copy (sub-objects are not copied) ({keyLabel(
					'Mod+D',
				)})"
			>
				<CopyPlus size={14} /> Duplicate
			</button>
			<button
				class="btn btn-ghost btn-sm"
				onclick={() => (copyOpen = true)}
				disabled={placeholder}
				title="Copy this object into another package"
			>
				<Copy size={14} /> Copy to…
			</button>
			<button
				class="btn btn-ghost btn-sm"
				onclick={deleteObject}
				disabled={placeholder}
				title="Turn the object (and its sub-objects) into empty placeholders and clear references"
			>
				<Trash2 size={14} />
				Delete…
			</button>
			<div class="flex-1"></div>
			<button
				class="btn btn-ghost btn-xs"
				onclick={showInFolder}
				title="Reveal the package file"
			>
				<Link2 size={12} /> file
			</button>
		</div>
		<div role="tablist" class="tabs-box tabs tabs-xs">
			{#if preview !== "none"}
				<button
					role="tab"
					class="tab {tab === 'preview' ? 'tab-active' : ''}"
					onclick={() => (tab = "preview")}>Preview</button
				>
			{/if}
			<button
				role="tab"
				class="tab {tab === 'properties' ? 'tab-active' : ''}"
				onclick={() => (tab = "properties")}>Properties</button
			>
			<button
				role="tab"
				class="tab {tab === 'native' ? 'tab-active' : ''}"
				onclick={() => (tab = "native")}>Native</button
			>
			<button
				role="tab"
				class="tab {tab === 'deps' ? 'tab-active' : ''}"
				onclick={() => (tab = "deps")}>References</button
			>
			<button
				role="tab"
				class="tab {tab === 'text' ? 'tab-active' : ''}"
				onclick={() => (tab = "text")}>Dump</button
			>
		</div>
	</div>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if studio.objectLoading}
			<div class="flex items-center gap-2 p-4 text-sm opacity-60">
				<span class="loading loading-sm loading-spinner"></span> Parsing…
			</div>
		{:else if tab === "preview"}
			{#key `${studio.pkg.path}#${row.i}`}
				{#if preview === "texture"}
					<TexturePreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "mesh"}
					<MeshPreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "anim"}
					<AnimPreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "material"}
					<MaterialPreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "wwise"}
					<WwisePreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "particle"}
					<ParticlePreview path={studio.pkg.path} index={row.i} />
				{:else if preview === "swf"}
					<SwfPreview path={studio.pkg.path} index={row.i} />
				{:else}
					<SoundPreview path={studio.pkg.path} index={row.i} />
				{/if}
			{/key}
		{:else if tab === "source"}
			{#key `${studio.pkg.path}#${row.i}`}
				<ScriptEditor path={studio.pkg.path} index={row.i} />
			{/key}
		{:else if tab === "properties"}
			{#if obj?.kind === "object"}
				<PropertyTree {obj} />
			{:else if obj?.kind === "script"}
				<pre class="selectable p-3 font-mono text-xs whitespace-pre-wrap">{obj.text}</pre>
				<p class="px-3 text-xs opacity-60">
					Script objects are edited with the UnrealScript compiler (uscript), not here.
				</p>
			{:else if obj?.kind === "raw"}
				<div class="p-3 text-sm">
					<p class="text-error">
						The header of this export could not be parsed: {obj.error}
					</p>
					<pre class="selectable mt-2 font-mono text-xs">{obj.size} bytes: {obj.hex}</pre>
				</div>
			{:else if obj?.kind === "empty"}
				<p class="p-3 text-sm opacity-60">
					Empty placeholder left by the cooker (or a deleted object).
				</p>
			{/if}
		{:else if tab === "native"}
			{#if obj?.kind === "object"}
				<div class="p-3">
					<div class="mb-2 text-xs opacity-70">
						native payload: <b>{obj.native.type}</b>
						{#if obj.native.type === "Raw"}· {bytes(obj.native.size)} not decoded{/if}
						· net index {obj.netIndex}
						{#if obj.hasRawValues}· <span class="text-warning"
								>some property values are raw</span
							>{/if}
					</div>
					{#if obj.stateFrame}
						<div class="mb-2 font-mono text-xs">
							state frame: node {obj.stateFrame.node.path} · state {obj.stateFrame
								.state.path} · stack {obj.stateFrame.stack}
						</div>
					{/if}
					{#if obj.component}
						<div class="mb-2 font-mono text-xs">
							component template: {obj.component.templateOwnerClass.path} · {obj
								.component.templateName ?? "-"}
						</div>
					{/if}
					<pre class="selectable font-mono text-xs whitespace-pre-wrap">{obj.native
							.text || "(no native data)"}</pre>
				</div>
			{:else}
				<p class="p-3 text-sm opacity-60">No native payload.</p>
			{/if}
		{:else if tab === "deps"}
			{#key `${studio.pkg.path}#${row.i}#${studio.tablesVersion}`}
				<DepsPanel path={studio.pkg.path} index={row.i} />
			{/key}
		{:else if tab === "text"}
			{#if text === null}
				<div class="p-3 text-sm opacity-60">Loading…</div>
			{:else}
				<pre class="selectable p-3 font-mono text-xs whitespace-pre-wrap">{text}</pre>
			{/if}
		{/if}
	</div>

	<RenameDialog bind:open={renameOpen} {row} />
	<CopyDialog bind:open={copyOpen} {row} />
	<ImportDialog bind:open={importOpen} {row} kind={preview} />
	<AnimExportDialog bind:open={animExportOpen} {row} />
{/if}
