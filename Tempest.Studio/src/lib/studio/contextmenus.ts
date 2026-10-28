// What the right-click menus offer for each kind of thing: packages, exports,
// tabs, recent entries. The components call `ui.openContextMenu(event, …)`
// with these lists; command ids keep their shortcut and enabled state.
import {
	AppWindow,
	ClipboardCopy,
	Copy,
	CopyPlus,
	Download,
	ExternalLink,
	FileCode2,
	FolderOpen,
	Pencil,
	PanelTopOpen,
	RotateCcw,
	Trash2,
	Upload,
	X,
} from "@lucide/svelte";
import { invoke } from "$lib/platform/api";
import { commands } from "./commands.svelte";
import { basename } from "./format";
import { session } from "./session.svelte";
import { studio } from "./state.svelte";
import { tabs } from "./tabs.svelte";
import { toasts } from "./toast.svelte";
import { ui, type ObjectAction } from "./ui.svelte";
import type { MenuItem } from "$lib/components/shell/MenuItems.svelte";

async function copyText(text: string, what: string): Promise<void> {
	try {
		await navigator.clipboard.writeText(text);
		toasts.push("success", text, `${what} copied`);
	} catch (error) {
		toasts.error(error, "Could not copy");
	}
}

/** Show a file in the system's file manager. */
export function revealItem(path: string): MenuItem {
	return {
		label: "Show in the file manager",
		icon: FolderOpen,
		run: () => invoke("shell:show-item", { path }),
	};
}

export function copyPathItem(path: string, label = "Copy the path"): MenuItem {
	return { label, icon: ClipboardCopy, run: () => copyText(path, "Path") };
}

/** A package of the package list (or a search result's package). */
export function packageMenu(path: string): MenuItem[] {
	const open = async (newTab: boolean) => {
		studio.mode = "packages";
		if (newTab) tabs.openInNewTab();
		await studio.openPackage(path);
	};
	const dirty = studio.dirty.includes(path);
	const items: MenuItem[] = [
		{ heading: basename(path) },
		{ label: "Open", icon: PanelTopOpen, run: () => open(false) },
		{ label: "Open in a new tab", icon: ExternalLink, run: () => open(true) },
	];
	if (dirty) {
		items.push(
			"-",
			{
				label: "Export a copy…",
				detail: "Writes the edited package elsewhere; the edits stay here",
				icon: Download,
				run: async () => {
					await open(false);
					commands.run("file.saveAs");
				},
			},
			{
				label: "Discard the edits…",
				detail: "Reloads it from the game folder (asks first)",
				icon: RotateCcw,
				danger: true,
				run: async () => {
					const ok = await ui.confirm({
						title: `Discard your edits to ${basename(path)}?`,
						message: session.project
							? "The edits are removed from the Studio and from the project."
							: "Every unsaved edit of this package is lost.",
						confirm: "Discard",
						danger: true,
					});
					if (ok === true) await studio.discardPaths([path]);
				},
			},
		);
	}
	items.push("-", copyPathItem(path), revealItem(path));
	return items;
}

/** An export of the open package: select it, then act like the inspector's buttons. */
export function exportMenu(index: number): MenuItem[] {
	const pkg = studio.pkg;
	const row = pkg?.exports[index];
	if (!pkg || !row) return [];
	const placeholder = row.size === 0 && row.classRef === 0;
	const act = (kind: ObjectAction) => async () => {
		studio.mode = "packages";
		if (studio.selected !== index) await studio.select(index);
		ui.objectAction = { kind, index };
	};
	return [
		{ heading: row.path },
		{
			label: "Open in a new tab",
			icon: ExternalLink,
			run: () => {
				tabs.openInNewTab();
				return studio.select(index);
			},
		},
		"-",
		{ label: "Export…", icon: Download, disabled: placeholder, run: act("export") },
		{ label: "Replace…", icon: Upload, disabled: placeholder, run: act("replace") },
		{ label: "Extract the raw data…", icon: FileCode2, run: act("raw") },
		"-",
		{ label: "Rename…", icon: Pencil, disabled: placeholder, run: act("rename") },
		{
			label: "Duplicate",
			detail: "A copy next to it, named …_copy",
			icon: CopyPlus,
			keys: "Mod+D",
			disabled: placeholder,
			run: async () => {
				if (studio.selected !== index) await studio.select(index);
				await studio.duplicate();
			},
		},
		{ label: "Copy to another package…", icon: Copy, disabled: placeholder, run: act("copy") },
		{ label: "Delete…", icon: Trash2, danger: true, disabled: placeholder, run: act("delete") },
		"-",
		{
			label: "Copy the object path",
			icon: ClipboardCopy,
			run: () => copyText(row.path, "Path"),
		},
		{
			label: "Copy the full path",
			detail: `${pkg.name}.${row.path}`,
			icon: ClipboardCopy,
			run: () => copyText(`${pkg.name}.${row.path}`, "Path"),
		},
	];
}

/** A tab of the tab bar. */
export function tabMenu(id: number): MenuItem[] {
	const i = tabs.list.findIndex((t) => t.id === id);
	return [
		{
			label: "Close",
			icon: X,
			keys: id === tabs.active ? "Mod+W" : undefined,
			run: () => tabs.close(id),
		},
		{
			label: "Close the other tabs",
			disabled: tabs.list.length < 2,
			run: () => tabs.closeOthers(id),
		},
		{
			label: "Close the tabs to the right",
			disabled: i < 0 || i === tabs.list.length - 1,
			run: () => tabs.closeToRight(id),
		},
		"-",
		{
			label: "Duplicate",
			icon: CopyPlus,
			keys: "Mod+T",
			run: async () => {
				await tabs.activate(id);
				tabs.duplicate();
			},
		},
	];
}

/** A recent project (welcome screen). */
export function recentProjectMenu(file: string): MenuItem[] {
	return [
		{ heading: basename(file).replace(/\.twrk$/i, "") },
		{ label: "Open", icon: PanelTopOpen, run: () => session.open(file) },
		"-",
		revealItem(file),
		copyPathItem(file),
		"-",
		{
			label: "Remove from the list",
			detail: "The project stays where it is",
			icon: X,
			run: () => session.forgetRecent(file),
		},
	];
}

/** A recent game folder (welcome screen). */
export function recentFolderMenu(dir: string): MenuItem[] {
	return [
		{ heading: dir },
		{ label: "Open", icon: PanelTopOpen, run: () => studio.openWorkspace(dir) },
		{ label: "Open in a new window", icon: AppWindow, run: () => studio.newWindow(dir) },
		"-",
		revealItem(dir),
		copyPathItem(dir),
		"-",
		{
			label: "Remove from the list",
			detail: "The folder stays where it is",
			icon: X,
			run: () => studio.forgetRecent(dir),
		},
	];
}
