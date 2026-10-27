// The Studio's commands (see commands.svelte.ts): registered once when the
// shell mounts. Dialogs open through `ui.open`, views through `studio.mode`.
import {
	AppWindow,
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
	Search,
	ShieldCheck,
	Sparkles,
	SunMoon,
	Undo2,
	Users,
	ZoomIn,
} from "@lucide/svelte";
import { pickDirectory, pickFile, pickSave, PACKAGE_FILTERS } from "$lib/platform/api";
import { assembly } from "./assembly.svelte";
import { CONTAINS_SCRIPT } from "./classes";
import { commands, type Command } from "./commands.svelte";
import { nav } from "./nav.svelte";
import { rpc } from "./rpc";
import { session } from "./session.svelte";
import { studio } from "./state.svelte";
import { tabs } from "./tabs.svelte";
import { toasts } from "./toast.svelte";
import { ui } from "./ui.svelte";

const ws = () => studio.workspace !== null;
const inAssembly = () => studio.mode === "assembly";
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

/** Discard asks once: the second run within a few seconds discards. */
let discardArmed = 0;
function discard(): void {
	if (Date.now() - discardArmed > 4000) {
		discardArmed = Date.now();
		toasts.push(
			"warning",
			"Run Discard again within 4 seconds to throw away the unsaved changes of this file.",
			"Discard",
		);
		return;
	}
	discardArmed = 0;
	if (inAssembly()) {
		if (assembly.doc) void assembly.discard(assembly.doc.id);
	} else {
		void studio.discard();
	}
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
			title: "Open workspace file…",
			group: "Workspace",
			keys: "Mod+Alt+O",
			icon: PackageOpen,
			global: true,
			run: () => session.pickAndOpen(),
		},
		{
			id: "workspace.save",
			title: "Save workspace",
			group: "Workspace",
			keys: "Mod+Alt+S",
			icon: Save,
			global: true,
			enabled: ws,
			run: () => (session.file ? session.save() : ui.open("saveWorkspace")),
		},
		{
			id: "workspace.saveAs",
			title: "Save workspace as…",
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
			title: "Save",
			group: "File",
			keys: "Mod+S",
			icon: Save,
			global: true,
			enabled: () => (inAssembly() ? assembly.dirtyDocs.length > 0 : studio.isDirty),
			run: () => (inAssembly() ? assembly.saveAll() : studio.save()),
		},
		{
			id: "file.saveAs",
			title: "Save as…",
			group: "File",
			keys: "Mod+Shift+S",
			icon: FolderOutput,
			global: true,
			enabled: () => (inAssembly() ? !!assembly.summary : !!studio.pkg),
			run: saveAs,
		},
		{
			id: "file.saveAll",
			title: "Save all changed files",
			group: "File",
			icon: SaveAll,
			enabled: () => studio.dirty.length > 0 || assembly.dirtyDocs.length > 0,
			run: async () => {
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
				inAssembly() ? assembly.undoLabels.length > 0 : studio.undoLabels.length > 0,
			run: () => (inAssembly() ? assembly.undo() : studio.undo()),
		},
		{
			id: "edit.discard",
			title: "Discard the changes of this file",
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
			id: "view.mod",
			title: "Mod cooker (package the changes as a .tempest)",
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
			keys: "Mod+Shift+F",
			icon: Search,
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
			id: "asm.newContent",
			title: "New device, card, item or champion…",
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
			id: "asm.exportMod",
			title: "Export a mod from the assembly changes…",
			group: "Assembly",
			icon: Package,
			enabled: () => !!assembly.summary,
			run: () => ui.open("modExport"),
		},
	];
	return commands.register(list);
}
