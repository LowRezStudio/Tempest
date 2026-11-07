// The Studio's commands (see commands.svelte.ts): registered once when the
// shell mounts. Dialogs open through `ui.open`, views through `studio.mode`.
import {
	AppWindow,
	Clapperboard,
	Box,
	Archive,
	Code2,
	Copy,
	Database,
	FileUp,
	FolderInput,
	Hammer,
	FolderOpen,
	FolderOutput,
	History,
	Keyboard,
	Music,
	Package,
	PackageOpen,
	Plug,
	Plus,
	RefreshCw,
	Replace,
	RotateCcw,
	Save,
	SaveAll,
	Settings,
	Search,
	ShieldCheck,
	Sparkles,
	Sun,
	SunMoon,
	Trash2,
	Undo2,
	Shirt,
	Map as MapIcon,
	Users,
	ZoomIn,
} from "@lucide/svelte";
import { invoke, pickDirectory, pickFile, pickSave, PACKAGE_FILTERS } from "$lib/platform/api";
import { assembly } from "./assembly.svelte";
import { CONTAINS_SCRIPT } from "./classes";
import { collision } from "./collision.svelte";
import { commands, type Command } from "./commands.svelte";
import { basename } from "./format";
import { lightBake } from "./lightbake.svelte";
import { map } from "./map.svelte";
import { matineeEd } from "./matinee.svelte";
import { nav } from "./nav.svelte";
import { rpc } from "./rpc";
import { session } from "./session.svelte";
import { studio } from "./state.svelte";
import { swfEditor } from "./swf.svelte";
import { tabs } from "./tabs.svelte";
import { toasts } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { updater } from "./updater.svelte";

const ws = () => studio.workspace !== null;
const inAssembly = () => studio.mode === "assembly";
const inMapScene = () => studio.mode === "map" && map.view !== "kismet";
const inPackages = () => studio.mode === "packages";

export async function openFolder(): Promise<void> {
	const dir = await pickDirectory(
		"Open the game's CookedPCConsole folder",
		studio.workspace?.dir,
	);
	if (dir) await studio.openWorkspace(dir);
}

export async function openPackageFile(): Promise<void> {
	const f = await pickFile("Open a package from anywhere", PACKAGE_FILTERS);
	if (f) await studio.openFiles([f]);
}

/**
 * Saving in place overwrites the game's own files: warn first (until the user
 * says not to), pointing at the ways that leave the game folder alone.
 * @param files The files that would be overwritten.
 * @returns Whether to go on.
 */
async function confirmInPlace(files: string[]): Promise<boolean> {
	const ok = await ui.confirm({
		title: "Overwrite the game's files?",
		message:
			"Saving in place changes your game install itself, which you rarely want: to keep your work, save a workspace file (or use a mod project); to share or test it, build a mod; to get the edited file, export a copy. Save in place only if you really mean to change the game folder.",
		items: files,
		confirm: "Save in the game folder",
		alt: "Save a workspace file instead",
		danger: true,
		remember: "save-in-place",
	});
	if (ok === "alt") {
		await commands.get("workspace.save")?.run();
		return false;
	}
	return ok === true;
}

/** The files Save would write: the open package's, or the changed .dat files in the Assembly view. */
const filesToSave = (all: boolean): string[] =>
	all
		? [...studio.dirty, ...assembly.dirtyDocs.map((d) => d.path)].map(basename)
		: inAssembly()
			? assembly.dirtyDocs.map((d) => basename(d.path))
			: studio.mode === "map"
				? map.dirtyLevels.map(basename)
				: studio.pkg
					? [studio.pkg.name]
					: [];

async function saveAs(): Promise<void> {
	if (inAssembly()) {
		const dir = await pickDirectory("Folder for the saved .dat files", studio.workspace?.dir);
		if (dir) await assembly.saveAll(dir);
		return;
	}
	if (!studio.pkg) return;
	const out = await pickSave("Save package as", studio.pkg.path, PACKAGE_FILTERS, "package");
	if (out) await studio.save(out);
}

async function exportSources(): Promise<void> {
	if (!studio.pkg) return;
	const path = studio.pkg.path;
	const outDir = await pickDirectory("Folder for <Package>/Classes/*.uc", studio.workspace?.dir);
	if (!outDir) return;
	try {
		const r = await studio.run("Decompiling…", () => rpc("script.export", { path, outDir }));
		toasts.push("success", `${r.classes} classes in ${r.dir}`, "Sources exported");
	} catch (error) {
		toasts.error(error, "Decompile failed");
	}
}

/** Every file with unsaved edits, as labels for confirmations. */
function unsavedFiles(): string[] {
	return [
		...studio.dirty.map((p) => basename(p)),
		...assembly.dirtyDocs.map((d) => basename(d.path)),
	];
}

/** Throw away the unsaved edits of the file being edited, after asking. */
async function discard(): Promise<void> {
	const name = inAssembly() ? assembly.doc && basename(assembly.doc.path) : studio.pkg?.name;
	if (!name) return;
	const ok = await ui.confirm({
		title: `Discard the changes of ${name}?`,
		message:
			"Every unsaved edit of this file is lost and it is reloaded from disk. Copies you exported and files saved in place are not touched.",
		confirm: "Discard changes",
		danger: true,
	});
	if (ok !== true) return;
	if (inAssembly()) {
		if (assembly.doc) await assembly.discard(assembly.doc.id);
	} else {
		await studio.discard();
	}
}

/** Throw away every unsaved edit of the window, after asking. */
async function discardAll(): Promise<void> {
	const files = unsavedFiles();
	if (files.length === 0) return;
	const ok = await ui.confirm({
		title: `Discard the changes of ${files.length} file${files.length === 1 ? "" : "s"}?`,
		message: "Every unsaved edit is lost and the files are reloaded from disk.",
		items: files,
		confirm: "Discard all",
		danger: true,
	});
	if (ok !== true) return;
	await studio.discardPaths([...studio.dirty]);
	for (const d of assembly.dirtyDocs) await assembly.discard(d.id);
	toasts.push("success", `${files.length} file(s) reloaded from disk`, "Changes discarded");
}

export function registerActions(): () => void {
	const list: Command[] = [
		// ----- File
		{
			id: "folder.open",
			title: "Open game folder…",
			group: "File",
			icon: FolderOpen,
			run: openFolder,
		},
		{
			id: "file.open",
			title: "Open package file…",
			group: "File",
			keys: "Mod+O",
			icon: FileUp,
			enabled: ws,
			run: openPackageFile,
		},
		{
			id: "window.new",
			title: "New window",
			group: "File",
			keys: "Mod+Shift+N",
			icon: AppWindow,
			global: true,
			run: () => studio.newWindow(),
		},
		{
			id: "workspace.open",
			title: "Open project…",
			detail: "A .twrk project or workspace file",
			group: "Workspace",
			keys: "Mod+Alt+O",
			icon: PackageOpen,
			global: true,
			run: () => session.pickAndOpen(),
		},
		{
			id: "workspace.save",
			detail: "Every edit and where you were, in a .twrk, for later",
			title: "Save as a workspace file",
			group: "Workspace",
			keys: "Mod+Alt+S",
			icon: Save,
			global: true,
			enabled: ws,
			run: () => (session.file ? session.save() : ui.open("saveWorkspace")),
		},
		{
			id: "workspace.saveAs",
			detail: "Choose the file and whether it is linked (small) or portable",
			title: "Save a copy of the workspace as…",
			group: "Workspace",
			icon: FolderOutput,
			enabled: ws,
			run: () => ui.open("saveWorkspace"),
		},
		{
			id: "workspace.recover",
			title: "Restore unsaved work of the last session",
			group: "Workspace",
			icon: History,
			enabled: () => session.recovery !== null,
			run: () => session.restoreRecovery(),
		},
		{
			id: "file.save",
			get title() {
				return session.project ? "Save project" : "Save into the game folder";
			},
			get detail() {
				return session.project
					? "Your edits are kept in the project (it also saves by itself); the game folder is not touched"
					: "Writes the open file in place; the Studio keeps its original for mod deltas";
			},
			group: "File",
			keys: "Mod+S",
			icon: Save,
			global: true,
			enabled: () =>
				session.project
					? true
					: inAssembly()
						? assembly.dirtyDocs.length > 0
						: studio.mode === "map"
							? map.dirtyLevels.length > 0
							: studio.isDirty,
			run: async () => {
				if (session.project) return session.saveProject();
				if (!(await confirmInPlace(filesToSave(false)))) return;
				if (studio.mode === "map") return studio.saveAll(map.dirtyLevels);
				await (inAssembly() ? assembly.saveAll() : studio.save());
			},
		},
		{
			id: "file.saveToGame",
			title: "Write the edits into the game folder",
			detail: "Overwrites the game's files with this file's edits (to test without building a mod)",
			group: "File",
			icon: FolderInput,
			enabled: () =>
				!!session.project &&
				(inAssembly() ? assembly.dirtyDocs.length > 0 : studio.isDirty),
			run: async () => {
				const name = inAssembly()
					? "the changed .dat files"
					: (studio.pkg?.name ?? "this file");
				const ok = await ui.confirm({
					title: `Write the edits into the game folder?`,
					message: `${name} in the game folder is overwritten with your edits. Your project and your mod don't need this: build the mod instead to test it cleanly.`,
					confirm: "Write into the game folder",
				});
				if (ok === true) await (inAssembly() ? assembly.saveAll() : studio.save());
			},
		},
		{
			id: "file.saveAs",
			detail: "Writes a copy elsewhere; the edits stay unsaved here and the mod cooker can use the copy",
			title: "Export a copy…",
			group: "File",
			keys: "Mod+Shift+S",
			icon: FolderOutput,
			global: true,
			enabled: () => (inAssembly() ? !!assembly.summary : !!studio.pkg),
			run: saveAs,
		},
		{
			id: "file.saveAll",
			detail: "Every file with unsaved edits, in place",
			title: "Save all into the game folder",
			group: "File",
			icon: SaveAll,
			enabled: () => studio.dirty.length > 0 || assembly.dirtyDocs.length > 0,
			run: async () => {
				if (!(await confirmInPlace(filesToSave(true)))) return;
				await studio.saveAll();
				if (assembly.dirtyDocs.length > 0) await assembly.saveAll();
			},
		},
		{
			id: "edit.undo",
			title: "Undo",
			group: "Edit",
			keys: "Mod+Z",
			icon: Undo2,
			enabled: () =>
				inAssembly()
					? assembly.undoLabels.length > 0
					: studio.mode === "map"
						? map.canUndo
						: !!swfEditor.doc?.canUndo || studio.undoLabels.length > 0,
			run: () => {
				if (inAssembly()) return assembly.undo();
				if (studio.mode === "map") return map.undo();
				if (swfEditor.doc?.canUndo) return swfEditor.doc.undoEdit();
				return studio.undo();
			},
		},
		{
			id: "edit.discard",
			detail: "Reloads this file from disk after asking",
			title: "Discard changes…",
			group: "Edit",
			icon: RotateCcw,
			enabled: () => (inAssembly() ? assembly.isDirty : studio.isDirty),
			run: discard,
		},
		// ----- Navigate
		{
			id: "nav.back",
			title: "Back",
			group: "Navigate",
			keys: "Alt+ArrowLeft",
			global: true,
			enabled: () => nav.canBack,
			run: () => nav.back(),
		},
		{
			id: "nav.forward",
			title: "Forward",
			group: "Navigate",
			keys: "Alt+ArrowRight",
			global: true,
			enabled: () => nav.canForward,
			run: () => nav.forward(),
		},
		{
			id: "palette.commands",
			title: "Command palette",
			group: "Navigate",
			keys: "Mod+K",
			global: true,
			run: () => ui.openPalette("commands"),
		},
		{
			id: "palette.commands2",
			title: "Command palette",
			group: "Navigate",
			keys: "Mod+Shift+P",
			global: true,
			hidden: true,
			run: () => ui.openPalette("commands"),
		},
		{
			id: "palette.open",
			title: "Go to package or table…",
			group: "Navigate",
			keys: "Mod+P",
			global: true,
			enabled: ws,
			run: () => ui.openPalette("open"),
		},
		{
			id: "palette.objects",
			title: "Go to object or row…",
			group: "Navigate",
			keys: "Mod+Shift+O",
			global: true,
			enabled: ws,
			run: () => ui.openPalette("objects"),
		},
		{
			id: "tab.close",
			title: "Close tab",
			group: "Tabs",
			keys: "Mod+W",
			global: true,
			enabled: () => tabs.active !== null,
			run: () => tabs.close(),
		},
		{
			id: "tab.duplicate",
			title: "Duplicate tab",
			group: "Tabs",
			keys: "Mod+T",
			global: true,
			enabled: () => tabs.active !== null,
			run: () => tabs.duplicate(),
		},
		{
			id: "tab.next",
			title: "Next tab",
			group: "Tabs",
			keys: "Ctrl+Tab",
			global: true,
			run: () => tabs.cycle(1),
		},
		{
			id: "tab.prev",
			title: "Previous tab",
			group: "Tabs",
			keys: "Ctrl+Shift+Tab",
			global: true,
			run: () => tabs.cycle(-1),
		},
		// ----- View
		{
			id: "view.packages",
			title: "Packages",
			group: "View",
			keys: "Mod+1",
			icon: Package,
			global: true,
			enabled: ws,
			run: () => (studio.mode = "packages"),
		},
		{
			id: "view.assembly",
			title: "Assembly (assembly.dat and languages)",
			group: "View",
			keys: "Mod+2",
			icon: Database,
			global: true,
			enabled: ws,
			run: () => (studio.mode = "assembly"),
		},
		{
			id: "view.map",
			title: "Maps (3D view, actors, Kismet)",
			group: "View",
			keys: "Mod+4",
			icon: MapIcon,
			global: true,
			enabled: ws,
			run: () => (studio.mode = "map"),
		},
		{
			id: "view.mod",
			title: "Build the mod",
			group: "View",
			keys: "Mod+3",
			icon: Hammer,
			global: true,
			enabled: ws,
			run: () => (studio.mode = "mod"),
		},
		{
			id: "view.sidebar",
			title: "Toggle side bar",
			group: "View",
			keys: "Mod+B",
			global: true,
			run: () => ui.toggle("side"),
		},
		{
			id: "view.details",
			title: "Toggle details pane",
			group: "View",
			keys: "Mod+J",
			global: true,
			run: () => ui.toggle("details"),
		},
		{
			id: "view.zoomIn",
			title: "Zoom in",
			group: "View",
			keys: "Mod+=",
			icon: ZoomIn,
			global: true,
			run: () => ui.setZoom(ui.zoom + 0.1),
		},
		{
			id: "view.zoomOut",
			title: "Zoom out",
			group: "View",
			keys: "Mod+-",
			global: true,
			run: () => ui.setZoom(ui.zoom - 0.1),
		},
		{
			id: "view.zoomReset",
			title: "Reset zoom",
			group: "View",
			keys: "Mod+0",
			global: true,
			run: () => ui.setZoom(1),
		},
		{
			id: "view.theme",
			title: "Toggle light / dark theme",
			group: "View",
			icon: SunMoon,
			run: () => (studio.theme = studio.theme === "mocha" ? "latte" : "mocha"),
		},
		{
			id: "help.shortcuts",
			title: "Keyboard shortcuts",
			group: "Help",
			keys: "F1",
			icon: Keyboard,
			global: true,
			run: () => ui.open("shortcuts"),
		},
		{
			id: "help.shortcuts2",
			title: "Keyboard shortcuts",
			group: "Help",
			keys: "Shift+?",
			hidden: true,
			run: () => ui.open("shortcuts"),
		},
		// ----- Package tools
		{
			id: "tools.search",
			title: "Search every package…",
			group: "Tools",
			keys: "Mod+F",
			icon: Search,
			global: true,
			enabled: ws,
			run: () => ui.open("search"),
		},
		{
			id: "tools.search2",
			title: "Search every package…",
			group: "Tools",
			keys: "Mod+Shift+F",
			hidden: true,
			global: true,
			enabled: ws,
			run: () => ui.open("search"),
		},
		{
			id: "tools.replace",
			title: "Find and replace in this package…",
			group: "Tools",
			keys: "Mod+H",
			icon: Replace,
			enabled: () => inPackages() && !!studio.pkg,
			run: () => ui.open("replace"),
		},
		{
			id: "package.new",
			title: "New package…",
			detail: "An empty package in the game folder, to add your own objects to",
			group: "Packages",
			icon: PackageOpen,
			enabled: ws,
			run: () => ui.open("newPackage"),
		},
		{
			id: "object.add",
			title: "Add to the package…",
			detail: "A new object from a file, a copy of an object from another package, or a group",
			group: "Packages",
			icon: Plus,
			enabled: () => inPackages() && !!studio.pkg,
			run: () => {
				const row = studio.selectedRow;
				ui.addGroup = row
					? row.class.toLowerCase() === "package"
						? row.i
						: row.outer > 0
							? row.outer - 1
							: null
					: null;
				ui.open("addObject");
			},
		},
		{
			id: "tools.batch",
			title: "Batch import a folder of files…",
			group: "Tools",
			icon: FolderInput,
			enabled: ws,
			run: () => ui.open("batch"),
		},
		{
			id: "tools.wwise",
			title: "Wwise audio…",
			group: "Tools",
			icon: Music,
			enabled: ws,
			run: () => ui.open("wwise"),
		},
		{
			id: "tools.verify",
			title: "Verify this package (round-trip every export)…",
			group: "Tools",
			icon: ShieldCheck,
			enabled: () => !!studio.pkg,
			run: () => ui.open("verify"),
		},
		{
			id: "tools.rewrite",
			title: "Rewrite this package compressed / uncompressed…",
			group: "Tools",
			icon: Archive,
			enabled: () => !!studio.pkg,
			run: () => ui.open("rewrite"),
		},
		{
			id: "tools.modFolder",
			title: "Prepare a mod folder from packages…",
			group: "Tools",
			icon: Package,
			enabled: ws,
			run: () => ui.open("modFolder"),
		},
		{
			id: "tools.exportSources",
			title: "Export the class sources of this script package…",
			group: "Tools",
			icon: Code2,
			enabled: () =>
				!!studio.pkg && (studio.pkg.summary.packageFlags & CONTAINS_SCRIPT) !== 0,
			run: exportSources,
		},
		{
			id: "app.settings",
			title: "Settings…",
			detail: "Updates, appearance, warnings, signing and publishing",
			group: "Help",
			keys: "Mod+,",
			icon: Settings,
			global: true,
			run: () => ui.open("settings"),
		},
		{
			id: "app.checkUpdates",
			title: "Check for updates",
			group: "Help",
			icon: RefreshCw,
			run: async () => {
				await updater.check();
				if (updater.status === "up-to-date") {
					toasts.push("success", "You have the newest Tempest Studio.", "Up to date");
				} else if (updater.status === "available") {
					ui.open("settings");
				}
			},
		},
		{
			id: "tools.mcp",
			title: "MCP server (let agents drive the Studio)…",
			group: "Tools",
			icon: Plug,
			run: () => ui.open("mcp"),
		},
		// ----- Assembly
		{
			id: "asm.reload",
			title: "Reload assembly.dat and the language files",
			group: "Assembly",
			icon: RefreshCw,
			enabled: () => !assembly.loading && ws(),
			run: () => assembly.open(true),
		},
		{
			id: "asm.newRow",
			title: "New row (fresh id, at the end)",
			group: "Assembly",
			keys: "Mod+Shift+Enter",
			icon: Plus,
			enabled: () => inAssembly() && !!assembly.page,
			run: () => assembly.addRow(null),
		},
		{
			id: "asm.duplicate",
			title: "Duplicate the selected row",
			group: "Assembly",
			keys: "Mod+D",
			icon: Copy,
			enabled: () => inAssembly() && assembly.selected !== null,
			run: () => assembly.selected !== null && assembly.addRow(assembly.selected),
		},
		{
			id: "map.addActor",
			title: "Add an actor to the map…",
			detail: "A copy of an actor of any level, where the view looks",
			group: "Map",
			keys: "Shift+A",
			icon: Plus,
			enabled: () => inMapScene() && map.levels.length > 0,
			run: () => {
				map.addingActor = true;
			},
		},
		{
			id: "map.collision",
			title: "Show collision in the map",
			detail: "Volumes' convex hulls and meshes' simple or complex collision, by what they block",
			group: "Map",
			icon: Box,
			enabled: () => inMapScene() && map.levels.length > 0,
			run: () => collision.toggle(),
		},
		{
			id: "map.duplicateActor",
			title: "Duplicate the selected actor",
			group: "Map",
			keys: "Mod+D",
			icon: Copy,
			enabled: () => inMapScene() && map.actor() !== null,
			run: () => map.duplicate(),
		},
		{
			id: "map.bakeLighting",
			title: "Bake lighting…",
			detail: "Static light maps for the meshes added or moved, the selection or whole levels",
			group: "Map",
			icon: Sun,
			enabled: () => inMapScene() && map.levels.length > 0,
			run: () => {
				lightBake.open = true;
			},
		},
		{
			id: "map.deleteActor",
			title: "Delete the selected actor",
			group: "Map",
			keys: "Delete",
			icon: Trash2,
			// In the Matinee view Delete removes the selected key instead.
			enabled: () =>
				inMapScene() && map.actor() !== null && !(map.view === "matinee" && map.matineeKey),
			run: () => map.removeSelected(),
		},
		{
			id: "map.newMatinee",
			title: "New Matinee",
			detail: "In the selected actor's level, moving it (or empty when nothing is selected)",
			group: "Map",
			icon: Clapperboard,
			enabled: () => studio.mode === "map" && !!map.path,
			run: () => {
				const actor = map.actor();
				const level = map.selected?.level ?? map.path!;
				const target = actor
					? actor.edit === "component"
						? actor.parent
						: actor.export
					: undefined;
				return matineeEd.create(level, { actor: target });
			},
		},
		{
			id: "map.openMatinee",
			title: "Open the level's Matinees",
			detail: "The first Matinee of the selected actor's level (the timeline lists the others)",
			group: "Map",
			icon: Clapperboard,
			enabled: () => studio.mode === "map" && !!map.path,
			run: () => matineeEd.openFirst(map.selected?.level ?? map.path!),
		},
		{
			id: "map.matineeKey",
			title: "Add a Matinee key at the playhead",
			detail: "On the selected track; a move key takes where its actor is",
			group: "Map",
			keys: "K",
			icon: Clapperboard,
			enabled: () =>
				studio.mode === "map" && map.view === "matinee" && matineeEd.track !== null,
			run: () => matineeEd.keyAtPlayhead(matineeEd.track!),
		},
		{
			id: "asm.newContent",
			title: "New device, card, item, skin or champion…",
			group: "Assembly",
			icon: Sparkles,
			enabled: () => !!assembly.summary,
			run: () => ui.open("newContent"),
		},
		{
			id: "asm.vendor",
			title: "Champion Vendor (which champions the client lists)…",
			group: "Assembly",
			icon: Users,
			enabled: () => !!assembly.summary,
			run: () => ui.open("vendor"),
		},
		{
			id: "asm.skins",
			title: "Skin collections (which skins the client lists per champion)…",
			group: "Assembly",
			icon: Shirt,
			enabled: () => !!assembly.summary,
			run: () => ui.open("skins"),
		},
		{
			id: "asm.exportMod",
			title: "Export a mod from the assembly changes…",
			group: "Assembly",
			icon: Package,
			enabled: () => !!assembly.summary,
			run: () => ui.open("modExport"),
		},
	];
	list.push(
		{
			id: "project.new",
			title: "New mod project…",
			detail: "Name your mod, pick the game folder and where to keep it",
			group: "Project",
			icon: Hammer,
			global: true,
			run: () => ui.open("newProject"),
		},
		{
			id: "project.close",
			title: "Close the project",
			detail: "Back to plain editing; its edits stay in the project and are unloaded (asks first)",
			group: "Project",
			enabled: () => !!session.project,
			run: () => session.closeProject(),
		},
		{
			id: "project.folder",
			title: "Show the project folder",
			group: "Project",
			icon: FolderOpen,
			enabled: () => !!session.project,
			run: () => session.project && invoke("shell:open-path", { path: session.project.dir }),
		},
	);
	list.push({
		id: "edit.discardAll",
		title: "Discard all changes…",
		detail: "Reloads every file with unsaved edits from disk after asking",
		group: "Edit",
		icon: RotateCcw,
		enabled: () => studio.dirty.length > 0 || assembly.dirtyDocs.length > 0,
		run: discardAll,
	});
	return commands.register(list);
}
