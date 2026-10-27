<!-- The title bar: workspace menu, Back/Forward, the command palette field,
	and the save actions of whatever is being edited. -->
<script lang="ts">
	import { ChevronDown, FolderOpen, Moon, Save, Search, Sun, Undo2 } from "@lucide/svelte";
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
	const unsaved = $derived(studio.dirty.length + assembly.dirtyDocs.length);
	const canUndo = $derived(
		inAssembly ? assembly.undoLabels.length > 0 : studio.undoLabels.length > 0,
	);
	const undoLabel = $derived(inAssembly ? assembly.undoLabels[0] : studio.undoLabels[0]);
	const canSave = $derived(inAssembly ? assembly.dirtyDocs.length > 0 : studio.isDirty);
	const saveTitle = $derived(
		inAssembly
			? `Save the changed .dat files in place (${keyLabel("Mod+S")})`
			: `Save ${studio.pkg?.name ?? "the package"} in place (${keyLabel("Mod+S")})`,
	);

	const workspaceItems = $derived<MenuItem[]>([
		"folder.open",
		"file.open",
		"window.new",
		"-",
		{
			heading: session.file
				? `Workspace file: ${basename(session.file.path)}`
				: "Workspace file",
		},
		"workspace.open",
		"workspace.save",
		"workspace.saveAs",
		...(session.recovery ? ["workspace.recover"] : []),
		...(studio.recents.length > 1 ? ["-" as const, { heading: "Recent game folders" }] : []),
		...studio.recents
			.filter((r) => r !== studio.workspace?.dir)
			.slice(0, 6)
			.map((r) => ({
				label:
					basename(r.replace(/[\\/](ChaosGame[\\/])?CookedPCConsole[\\/]?$/i, "")) || r,
				detail: r,
				run: () => studio.openWorkspace(r),
			})),
	]);
</script>

<header
	class="border-base-300 bg-base-200 flex h-10 shrink-0 items-center gap-1 border-b pr-2 pl-1"
>
	<img src="/studio-icon.png" alt="" class="mx-1 h-6 w-6" />
	<Menu
		items={workspaceItems}
		buttonClass="btn btn-ghost btn-sm max-w-64 gap-1.5 px-2"
		title={studio.workspace?.dir}
	>
		<FolderOpen size={14} class="shrink-0" />
		<span class="truncate font-semibold">{gameName}</span>
		{#if session.file}
			<span class="truncate font-normal opacity-60 max-lg:hidden"
				>· {basename(session.file.path)}</span
			>
		{/if}
		<ChevronDown size={12} class="shrink-0 opacity-60" />
	</Menu>
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
	{#if unsaved > 0}
		<span
			class="badge badge-sm badge-warning shrink-0"
			title="{studio.dirty.length} package(s) and {assembly.dirtyDocs
				.length} .dat file(s) unsaved"
			>{unsaved}<span class="max-lg:hidden">&nbsp;unsaved</span></span
		>
	{/if}
	<div class="join shrink-0">
		<button
			class="btn btn-primary btn-sm join-item"
			onclick={() => commands.run("file.save")}
			disabled={!canSave}
			title={saveTitle}
		>
			<Save size={14} /><span class="max-md:hidden">Save</span>
		</button>
		<Menu
			items={[
				"file.save",
				"file.saveAs",
				"file.saveAll",
				"-",
				"workspace.save",
				"workspace.saveAs",
				"-",
				"edit.discard",
			]}
			buttonClass="btn btn-primary btn-sm join-item px-1.5"
			title="More save actions"
			align="end"
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
