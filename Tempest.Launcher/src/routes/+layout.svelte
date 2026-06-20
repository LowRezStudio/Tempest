<script lang="ts">
	import "$lib/styles/global.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { platform } from "@tauri-apps/plugin-os";
	import { page } from "$app/state";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import AppCloseLobbyWizard from "$lib/components/lobby/AppCloseLobbyWizard.svelte";
	import InstallModOverlay from "$lib/components/mods/InstallModOverlay.svelte";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import ReplaceModDialog from "$lib/components/mods/ReplaceModDialog.svelte";
	import HostServerWizard from "$lib/components/server-list/HostServerWizard.svelte";
	import JoinServerWizard from "$lib/components/server-list/JoinServerWizard.svelte";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import { installMod } from "$lib/core/mods";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import { clearStaleConnectionIfNeeded } from "$lib/lobby/stores";
	import { confirmReplaceMod, replaceDialogStore, resolveReplaceMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance";
	import { theme } from "$lib/stores/settings";
	import {
		addToast,
		appCloseLobbyWizardOpen,
		hostServerWizardOpen,
		instanceWizardOpen,
		joinServerWizardOpen,
	} from "$lib/stores/ui";
	import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";

	clearStaleConnectionIfNeeded();

	const { children } = $props();
	const queryClient = new QueryClient();

	let isDraggingFiles = $state(false);
	let showInstanceSelect = $state(false);

	let droppedFilePath = $state("");
	let targetInstance = $state<any>(null);

	async function handleModFileDrop(filePath: string) {
		const ext = filePath.split(".").pop()?.toLowerCase();
		if (ext !== "upk" && ext !== "pck") {
			addToast({
				title: m.toast_unsupported_file_title(),
				message: m.toast_unsupported_file_message(),
				tone: "error",
			});
			return;
		}

		droppedFilePath = filePath;

		const pathname = page.url.pathname;
		const match = pathname.match(/^\/instance\/([^/]+)/);
		if (match) {
			const instanceId = match[1];
			const inst = $instanceMap[instanceId];
			if (inst && inst.state?.type === "prepared") {
				targetInstance = inst;
				await proceedWithInstall();
				return;
			}
		}

		showInstanceSelect = true;
	}

	async function proceedWithInstall() {
		if (!targetInstance || !droppedFilePath) return;

		try {
			let res = await installMod(targetInstance.path, droppedFilePath, false);
			if (res.Conflict) {
				const modName = droppedFilePath.split(/[/\\]/).pop() || droppedFilePath;
				const confirmed = await confirmReplaceMod(modName, res.IsModConflict);
				if (confirmed) {
					res = await installMod(targetInstance.path, droppedFilePath, true);
				} else {
					return; // Cancelled
				}
			}

			if (res.Success) {
				addToast({
					title: m.toast_mod_installed_title(),
					message: m.toast_mod_installed_message({
						name: res.Mod?.Name ?? m.toast_mod_installed_fallback(),
					}),
					tone: "success",
				});
				queryClient.invalidateQueries({ queryKey: ["mods", targetInstance.path] });
			} else {
				addToast({
					title: m.toast_installation_failed_title(),
					message: res.Message || m.toast_installation_failed_unknown(),
					tone: "error",
				});
			}
		} catch (error: any) {
			addToast({
				title: m.toast_installation_failed_title(),
				message: error.message || m.toast_installation_failed_internal(),
				tone: "error",
			});
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
					appCloseLobbyWizardOpen.set(true);
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
						void handleModFileDrop(paths[0]);
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

	$effect(() => {
		const currentTheme = $theme;
		if (currentTheme === "system") {
			document.documentElement.removeAttribute("data-theme");
		} else {
			document.documentElement.setAttribute("data-theme", currentTheme);
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
</script>

<QueryClientProvider client={queryClient}>
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
		<InstanceWizard bind:open={$instanceWizardOpen} />
		<HostServerWizard bind:open={$hostServerWizardOpen} />
		<JoinServerWizard bind:open={$joinServerWizardOpen} />
		<AppCloseLobbyWizard bind:open={$appCloseLobbyWizardOpen} />
		<InstanceSelectModal
			bind:open={showInstanceSelect}
			onselect={handleInstanceSelected}
			oncancel={() => {}}
		/>
		<ReplaceModDialog
			bind:open={$replaceDialogStore.open}
			modName={$replaceDialogStore.modName}
			isModConflict={$replaceDialogStore.isModConflict}
			onconfirm={() => resolveReplaceMod(true)}
			oncancel={() => resolveReplaceMod(false)}
		/>
	</div>
	<ToastStack />
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
