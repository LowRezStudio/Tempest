<script lang="ts">
	import "$lib/styles/global.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import { Tooltip } from "bits-ui";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { platform } from "@tauri-apps/plugin-os";
	import { page } from "$app/state";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import AppCloseLobbyWizard from "$lib/components/lobby/AppCloseLobbyWizard.svelte";
	import InstallModOverlay from "$lib/components/mods/InstallModOverlay.svelte";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import ReplaceModDialog from "$lib/components/mods/ReplaceModDialog.svelte";
	import UnverifiedModDialog from "$lib/components/mods/UnverifiedModDialog.svelte";
	import HostServerWizard from "$lib/components/server-list/HostServerWizard.svelte";
	import JoinServerWizard from "$lib/components/server-list/JoinServerWizard.svelte";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import UpdateDialog from "$lib/components/updater/UpdateDialog.svelte";
	import { installMod } from "$lib/core/mods";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import { clearStaleConnectionIfNeeded } from "$lib/lobby/stores.svelte";
	import {
		confirmReplaceMod,
		confirmUnverifiedMod,
		replaceDialogStore,
		resolveReplaceMod,
		resolveUnverifiedMod,
		unverifiedDialogStore,
	} from "$lib/mods/ui.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { customThemeCss, theme } from "$lib/stores/settings.svelte";
	import {
		addToast,
		appCloseLobbyWizardOpen,
		hostServerWizardOpen,
		instanceWizardOpen,
		joinServerWizardOpen,
		removeToast,
	} from "$lib/stores/ui.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import { localeState } from "$lib/stores/locale.svelte";
	import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";

	clearStaleConnectionIfNeeded();

	const { children } = $props();
	const queryClient = new QueryClient();

	let isDraggingFiles = $state(false);
	let showInstanceSelect = $state(false);

	let droppedFilePaths = $state<string[]>([]);
	let targetInstance = $state<any>(null);

	async function handleModFileDrop(filePaths: string[]) {
		const validPaths: string[] = [];
		let hadInvalid = false;

		for (const filePath of filePaths) {
			const ext = filePath.split(".").pop()?.toLowerCase();
			if (ext === "upk" || ext === "pck" || ext === "zip" || ext === "tempest") {
				validPaths.push(filePath);
			} else {
				hadInvalid = true;
			}
		}

		if (hadInvalid) {
			addToast({
				title: m.toast_unsupported_file_title(),
				message: m.toast_unsupported_file_message(),
				tone: "error",
			});
		}

		if (validPaths.length === 0) return;

		droppedFilePaths = validPaths;

		const pathname = page.url.pathname;
		const match = pathname.match(/^\/instance\/([^/]+)/);
		if (match) {
			const instanceId = match[1];
			const inst = instanceMap.value[instanceId];
			if (inst && inst.state?.type === "prepared") {
				targetInstance = inst;
				await proceedWithInstall();
				return;
			}
		}

		showInstanceSelect = true;
	}

	async function proceedWithInstall() {
		if (!targetInstance || droppedFilePaths.length === 0) return;

		let successCount = 0;
		let lastInstalledName = "";

		for (const filePath of droppedFilePaths) {
			const modFileName = filePath.split(/[/\\]/).pop() || filePath;
			let installingToastId: string | undefined;
			try {
				installingToastId = addToast({
					title: m.toast_mod_installing_title(),
					message: m.toast_mod_installing_message({ name: modFileName }),
					tone: "info",
					duration: 0,
				});

				let allowedUnsigned = false;
				let res = await installMod(targetInstance.path, filePath, false, false);
				if (res.Unverified) {
					const confirmed = await confirmUnverifiedMod(modFileName);
					if (confirmed) {
						allowedUnsigned = true;
						res = await installMod(targetInstance.path, filePath, false, true);
					} else {
						if (installingToastId) removeToast(installingToastId);
						continue; // Skip this one on cancel
					}
				}

				if (res.Conflict) {
					const confirmed = await confirmReplaceMod(modFileName, res.IsModConflict);
					if (confirmed) {
						res = await installMod(
							targetInstance.path,
							filePath,
							true,
							allowedUnsigned,
						);
					} else {
						if (installingToastId) removeToast(installingToastId);
						continue; // Skip this one on cancel
					}
				}

				if (installingToastId) removeToast(installingToastId);

				if (res.Success) {
					successCount++;
					lastInstalledName =
						res.Mod?.Name ?? modFileName ?? m.toast_mod_installed_fallback();
				} else {
					addToast({
						title: m.toast_installation_failed_title(),
						message: `${modFileName}: ${res.Message || m.toast_installation_failed_unknown()}`,
						tone: "error",
					});
				}
			} catch (error: any) {
				if (installingToastId) removeToast(installingToastId);
				addToast({
					title: m.toast_installation_failed_title(),
					message: `${modFileName}: ${error.message || m.toast_installation_failed_internal()}`,
					tone: "error",
				});
			}
		}

		if (successCount > 0) {
			if (successCount === 1) {
				addToast({
					title: m.toast_mod_installed_title(),
					message: m.toast_mod_installed_message({
						name: lastInstalledName,
					}),
					tone: "success",
				});
			} else {
				addToast({
					title: m.toast_mod_installed_title(),
					message: `Successfully installed ${successCount} mods`,
					tone: "success",
				});
			}
			queryClient.invalidateQueries({ queryKey: ["mods", targetInstance.path] });
		}
	}

	async function handleInstanceSelected(inst: any) {
		targetInstance = inst;
		showInstanceSelect = false;
		await proceedWithInstall();
	}

	$effect(() => {
		const appWindow = getCurrentWindow();
		let unlisten: (() => void) | undefined;
		appWindow
			.onCloseRequested(async (event) => {
				event.preventDefault();
				if (lobbyManager.isConnected()) {
					appCloseLobbyWizardOpen.value = true;
				} else {
					await appWindow.destroy();
				}
			})
			.then((fn) => {
				unlisten = fn;
			});
		return () => unlisten?.();
	});

	$effect(() => {
		let unlistenDrop: (() => void) | undefined;
		const appWindow = getCurrentWindow();
		appWindow
			.onDragDropEvent((event) => {
				if (event.payload.type === "enter" || event.payload.type === "over") {
					isDraggingFiles = true;
				} else if (event.payload.type === "drop") {
					isDraggingFiles = false;
					const paths = event.payload.paths;
					if (paths && paths.length > 0) {
						void handleModFileDrop(paths);
					}
				} else if (event.payload.type === "leave") {
					isDraggingFiles = false;
				}
			})
			.then((fn) => {
				unlistenDrop = fn;
			});
		return () => unlistenDrop?.();
	});

	let customThemeStyle: HTMLStyleElement | undefined;

	function transformCustomTheme(input: string): string {
		if (!input.includes('@plugin "daisyui/theme"')) return input;
		const lines = input.split("\n");
		const vars: string[] = [];
		for (const line of lines) {
			const trimmed = line.trim();
			if (trimmed.startsWith("--") || trimmed.startsWith("color-scheme:")) {
				vars.push(trimmed.replaceAll('"', ""));
			}
		}
		return vars.length ? `[data-theme="custom"] {\n${vars.join("\n")}\n}` : input;
	}

	$effect(() => {
		const currentTheme = theme.value;
		if (currentTheme === "system") {
			document.documentElement.removeAttribute("data-theme");
		} else {
			document.documentElement.setAttribute("data-theme", currentTheme);
		}

		if (currentTheme === "custom") {
			const raw = customThemeCss.value;
			const css = raw ? transformCustomTheme(raw) : "";
			if (css) {
				if (!customThemeStyle) {
					customThemeStyle = document.createElement("style");
					customThemeStyle.id = "custom-theme";
					document.head.append(customThemeStyle);
				}
				customThemeStyle.textContent = css;
			}
		} else {
			const existing = document.querySelector("#custom-theme");
			if (existing) {
				existing.remove();
				customThemeStyle = undefined;
			}
		}
	});

	$effect(() => {
		polyfillCountryFlagEmojis();
		document.documentElement.dataset.platform = platform();

		const handleContextMenu = (event: Event) => {
			if (!import.meta.env.DEV) {
				event.preventDefault();
			}
		};
		const handleDragStart = (event: Event) => {
			event.preventDefault();
		};
		const handleDrop = (event: Event) => {
			event.preventDefault();
		};

		document.addEventListener("contextmenu", handleContextMenu);
		document.addEventListener("dragstart", handleDragStart);
		document.addEventListener("drop", handleDrop);

		return () => {
			document.removeEventListener("contextmenu", handleContextMenu);
			document.removeEventListener("dragstart", handleDragStart);
			document.removeEventListener("drop", handleDrop);
		};
	});

	$effect(() => {
		// ponytail: check for updates silently on startup
		updaterStore.checkForUpdates(true);
	});
</script>

<QueryClientProvider client={queryClient}>
	<Tooltip.Provider delayDuration={400}>
		{#key localeState.current}
			<div class="flex h-screen w-full overflow-hidden">
				<Sidebar />
				<main class="flex-1 min-w-0 relative overflow-hidden">
					{#key page.url.pathname}
						<div class="page-transition">
							{@render children?.()}
						</div>
					{/key}
					<InstallModOverlay visible={isDraggingFiles} />
				</main>
				<InstanceWizard bind:open={instanceWizardOpen.value} />
				<HostServerWizard bind:open={hostServerWizardOpen.value} />
				<JoinServerWizard bind:open={joinServerWizardOpen.value} />
				<AppCloseLobbyWizard bind:open={appCloseLobbyWizardOpen.value} />
				<InstanceSelectModal
					bind:open={showInstanceSelect}
					onselect={handleInstanceSelected}
					oncancel={() => {}}
				/>
				<ReplaceModDialog
					bind:open={replaceDialogStore.value.open}
					modName={replaceDialogStore.value.modName}
					onconfirm={() => resolveReplaceMod(true)}
					oncancel={() => resolveReplaceMod(false)}
				/>
				<UnverifiedModDialog
					bind:open={unverifiedDialogStore.value.open}
					modName={unverifiedDialogStore.value.modName}
					onconfirm={() => resolveUnverifiedMod(true)}
					oncancel={() => resolveUnverifiedMod(false)}
				/>
				<UpdateDialog />
			</div>
			<ToastStack />
		{/key}
	</Tooltip.Provider>
</QueryClientProvider>

<style>
	.page-transition {
		position: absolute;
		inset: 0;
		overflow-y: auto;
		animation: page-enter 250ms var(--ease-snappy) both;
	}

	:global(.page-transition:has(~ .page-transition)) {
		animation: page-exit 250ms var(--ease-smooth) both;
	}
</style>
