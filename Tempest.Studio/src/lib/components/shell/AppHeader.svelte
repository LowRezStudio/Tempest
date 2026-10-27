<!-- The title bar: the project (or game folder) menu with its save state,
	Back/Forward, the palette field, and the actions for what is being edited.
	In a mod project the edits are kept in the project, which saves itself;
	the main button builds the mod. Without a project, Save writes into the
	game folder. -->
<script lang="ts">
	import {
		Check,
		ChevronDown,
		CircleAlert,
		FolderOpen,
		Hammer,
		Moon,
		RotateCcw,
		Save,
		Search,
		Sun,
		Undo2,
	} from "@lucide/svelte";
	import NavButtons from "$lib/components/NavButtons.svelte";
	import Menu, { type MenuItem } from "$lib/components/shell/Menu.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { commands, keyLabel } from "$lib/studio/commands.svelte";
	import { basename } from "$lib/studio/format";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	const inAssembly = $derived(studio.mode === "assembly");
	const gameName = $derived(
		studio.workspace
			? basename(
					studio.workspace.dir.replace(
						/[\\/](ChaosGame[\\/])?CookedPCConsole[\\/]?$/i,
						"",
					),
				)
			: "",
	);
	const edited = $derived(studio.dirty.length + assembly.dirtyDocs.length);
	const canUndo = $derived(
		inAssembly ? assembly.undoLabels.length > 0 : studio.undoLabels.length > 0,
	);
	const undoLabel = $derived(inAssembly ? assembly.undoLabels[0] : studio.undoLabels[0]);
	const fileEdited = $derived(inAssembly ? assembly.dirtyDocs.length > 0 : studio.isDirty);

	/** The edited files, each opening its file; then the bulk actions. */
	const editedItems = $derived<MenuItem[]>([
		{
			heading: session.project
				? "Edited files (kept in your project)"
				: "Files with unsaved edits",
		},
		...studio.dirty.map((p) => ({
			label: basename(p),
			detail: p,
			run: () => {
				studio.mode = "packages";
				return studio.openPackage(p);
			},
		})),
		...assembly.dirtyDocs.map((d) => ({
			label: basename(d.path),
			detail: d.path,
			run: () => {
				studio.mode = "assembly";
				const t = d.tables[0];
				return t ? assembly.openTable(d.id, t.name, []) : undefined;
			},
		})),
		"-",
		...(session.project ? ["view.mod"] : ["file.saveAll", "workspace.save"]),
		"edit.discardAll",
	]);

	const recentProjects = $derived(
		session.recents
			.filter((r) => r !== session.file?.path)
			.slice(0, 5)
			.map((r) => ({
				label: basename(r).replace(/\.twrk$/i, ""),
				detail: r,
				run: () => session.open(r),
			})),
	);

	const mainItems = $derived<MenuItem[]>([
		...(session.project
			? [
					{ heading: `Project ${session.project.name}` },
					"project.folder",
					"project.close",
					"-",
				]
			: []),
		"project.new",
		"workspace.open",
		...(recentProjects.length > 0 ? [{ heading: "Recent projects" }, ...recentProjects] : []),
		"-",
		{ heading: `Game folder: ${gameName}` },
		"folder.open",
		"file.open",
		"window.new",
	]);

	const saveItems = $derived<MenuItem[]>(
		session.project
			? [
					"file.save",
					"file.saveToGame",
					"file.saveAs",
					"-",
					"edit.discard",
					"edit.discardAll",
				]
			: [
					"file.save",
					"file.saveAs",
					"file.saveAll",
					"-",
					"workspace.save",
					"workspace.saveAs",
					"-",
					"edit.discard",
					"edit.discardAll",
				],
	);

	const stateText = $derived(
		session.projectState === "saved"
			? "Saved"
			: session.projectState === "saving"
				? "Saving…"
				: session.projectState === "pending"
					? "Edited"
					: "Not saved",
	);
</script>

<header
	class="border-base-300 bg-base-200 flex h-10 shrink-0 items-center gap-1 border-b pr-2 pl-1"
>
	<img src="/studio-icon.png" alt="" class="mx-1 h-6 w-6" />
	<Menu
		items={mainItems}
		buttonClass="btn btn-ghost btn-sm max-w-72 gap-1.5 px-2"
		title={session.project ? session.project.dir : studio.workspace?.dir}
		width="w-80"
	>
		{#if session.project}
			<Hammer size={14} class="text-primary shrink-0" />
			<span class="truncate font-semibold">{session.project.name}</span>
		{:else}
			<FolderOpen size={14} class="shrink-0" />
			<span class="truncate font-semibold">{gameName}</span>
		{/if}
		<ChevronDown size={12} class="shrink-0 opacity-60" />
	</Menu>
	{#if session.project}
		<span
			class="flex shrink-0 items-center gap-1 text-xs {session.projectState === 'error'
				? 'text-error'
				: 'opacity-60'} max-md:hidden"
			title={session.projectState === "error"
				? "The project could not be saved; press Ctrl+S to retry"
				: "Your edits are kept in the project file, which saves itself"}
		>
			{#if session.projectState === "saved"}<Check
					size={12}
				/>{:else if session.projectState === "error"}<CircleAlert size={12} />{:else}<span
					class="loading loading-xs loading-spinner"
				></span>{/if}
			{stateText}
		</span>
	{/if}
	<NavButtons />
	<div class="flex min-w-0 flex-1 justify-center px-2">
		<button
			class="border-base-300 bg-base-100 hover:border-primary/50 flex h-7 w-full max-w-xl min-w-0 items-center gap-2 rounded-md border px-2 text-left text-xs opacity-80 hover:opacity-100"
			onclick={() => ui.openPalette("open")}
			title="Go to a package or table, run a command (>) or find an object (@)"
		>
			<Search size={13} class="shrink-0" />
			<span class="truncate">Go to package, table, object… or run a command</span>
			<span class="flex-1"></span>
			<kbd class="kbd kbd-xs max-sm:hidden">{keyLabel("Mod+P")}</kbd>
			<kbd class="kbd kbd-xs max-md:hidden">{keyLabel("Mod+K")}</kbd>
		</button>
	</div>
	<button
		class="btn btn-ghost btn-sm btn-square"
		onclick={() => commands.run("edit.undo")}
		disabled={!canUndo}
		title={canUndo ? `Undo: ${undoLabel} (${keyLabel("Mod+Z")})` : "Nothing to undo"}
		aria-label="Undo"
	>
		<Undo2 size={15} />
	</button>
	{#if edited > 0}
		<Menu
			items={editedItems}
			buttonClass="badge {session.project
				? 'badge-ghost'
				: 'badge-warning'} cursor-pointer gap-1 font-medium"
			title={session.project
				? "Files you edited: kept in your project, not written into the game folder"
				: "Files with unsaved edits"}
			align="end"
			width="w-80"
		>
			{edited}<span class="max-lg:hidden">{session.project ? "edited" : "unsaved"}</span>
			<ChevronDown size={11} />
		</Menu>
	{/if}
	<button
		class="btn btn-ghost btn-sm shrink-0"
		onclick={() => commands.run("edit.discard")}
		disabled={!fileEdited}
		title="Throw away your edits to this file (asks first)"
	>
		<RotateCcw size={14} /><span class="max-xl:hidden">Discard</span>
	</button>
	<div class="join shrink-0">
		{#if session.project}
			<button
				class="btn btn-primary btn-sm join-item"
				onclick={() => (studio.mode = "mod")}
				title="Build the mod from your edits"
			>
				<Hammer size={14} /><span class="max-md:hidden">Build mod</span>
			</button>
		{:else}
			<button
				class="btn btn-primary btn-sm join-item"
				onclick={() => commands.run("file.save")}
				disabled={!fileEdited}
				title="Save into the game folder ({keyLabel('Mod+S')})"
			>
				<Save size={14} /><span class="max-md:hidden">Save</span>
			</button>
		{/if}
		<Menu
			items={saveItems}
			buttonClass="btn btn-primary btn-sm join-item px-1.5"
			title="More"
			align="end"
			width="w-96"
		>
			<ChevronDown size={14} />
		</Menu>
	</div>
	<button
		class="btn btn-square btn-ghost btn-sm"
		onclick={() => (studio.theme = studio.theme === "mocha" ? "latte" : "mocha")}
		title="Toggle light / dark theme"
		aria-label="Toggle theme"
	>
		{#if studio.theme === "mocha"}<Sun size={14} />{:else}<Moon size={14} />{/if}
	</button>
</header>
