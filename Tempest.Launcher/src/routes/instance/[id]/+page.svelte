<script lang="ts">
	import {
		Box,
		File,
		Folder,
		Gamepad2,
		PackageOpen,
		Play,
		RefreshCw,
		Settings,
		Square,
		Terminal,
		Trash2,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import InstanceMenu from "$lib/components/library/InstanceMenu.svelte";
	import ModMenu from "$lib/components/mods/ModMenu.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installMod } from "$lib/core/mods";
	import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createModsQuery, createRemoveModMutation } from "$lib/queries/mods";
	import { instanceMap } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { addToast, removeToast } from "$lib/stores/ui";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import MarkdownIt from "markdown-it";
	import type { ModRecord } from "$lib/core/mods";

	let activeTab = $state<"content">("content");

	const instance = $derived($instanceMap[page.params.id!]);
	let isSettingUp = $derived(
		(instance?.state as { type?: string } | undefined)?.type === "setup",
	);

	const queryClient = useQueryClient();

	async function handleInstallMod() {
		if (!instance?.path) return;
		try {
			const result = await openDialog({
				directory: false,
				multiple: true,
				title: m.dialog_select_mod_files_title(),
				filters: [
					{
						name: m.dialog_select_mod_files_filter(),
						extensions: ["upk", "pck", "zip", "tempest"],
					},
				],
			});

			if (result) {
				const paths = Array.isArray(result) ? result : [result];
				let successCount = 0;
				let lastInstalledName = "";

				for (const filePath of paths) {
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
						let res = await installMod(instance.path, filePath, false, false);
						if (res.Unverified) {
							const confirmed = await confirmUnverifiedMod(modFileName);
							if (confirmed) {
								allowedUnsigned = true;
								res = await installMod(instance.path, filePath, false, true);
							} else {
								if (installingToastId) removeToast(installingToastId);
								continue; // Skip this one on cancel
							}
						}

						if (res.Conflict) {
							const confirmed = await confirmReplaceMod(
								modFileName,
								res.IsModConflict,
							);
							if (confirmed) {
								res = await installMod(
									instance.path,
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
					queryClient.invalidateQueries({ queryKey: ["mods", instance.path] });
				}
			}
		} catch (error: any) {
			addToast({
				title: m.toast_installation_failed_title(),
				message: error.message || m.toast_installation_failed_internal(),
				tone: "error",
			});
		}
	}

	const modsQuery = createModsQuery(() => instance?.path ?? "");
	let modsList = $derived(modsQuery.data ?? []);
	let isQueryLoading = $derived(modsQuery.isFetching);

	const removeModMutation = createRemoveModMutation();

	function handleRemoveMod(modName: string) {
		if (!instance) return;
		removeModMutation.mutate({ gamePath: instance.path, modName });
	}

	function handleRefreshMods() {
		modsQuery.refetch();
	}

	$effect(() => {
		if (!instance) {
			goto("/library");
		}
	});

	let isSettingsModalOpen = $state(false);

	// Check if this instance is currently running
	let isRunning = $derived($processesList.some((p) => p.instance?.id === instance?.id));
	const launchGameMutation = createLaunchGameMutation();
	const killGameMutation = createKillGameMutation();
	let isLaunching = $derived(launchGameMutation.isPending);
	let isKilling = $derived(killGameMutation.isPending);
	let launchError = $derived(launchGameMutation.error?.message ?? "");
	let killError = $derived(killGameMutation.error?.message ?? "");

	function handleLaunchToggle() {
		if (!instance) return;
		if (isRunning) {
			killGameMutation.mutate(instance);
			return;
		}
		launchGameMutation.mutate(instance);
	}

	function clearLaunchError() {
		launchGameMutation.reset();
	}

	function clearKillError() {
		killGameMutation.reset();
	}

	const md = new MarkdownIt({ html: true, linkify: true });

	let isFilesDialogOpen = $state(false);
	let selectedModForFiles = $state<ModRecord | null>(null);
	let modDetailsTab = $state<"details" | "readme" | "files">("details");
	let readmeContent = $state<string>("");

	let isReadmeMarkdown = $derived(
		selectedModForFiles?.Readme ?
			selectedModForFiles.Readme.toLowerCase().endsWith(".md")
		:	false,
	);

	function handleOpenFiles(mod: ModRecord) {
		selectedModForFiles = mod;
		modDetailsTab = "details";
		isFilesDialogOpen = true;
		readmeContent = "";

		if (mod.ReadmeContent && mod.Readme && mod.Readme.toLowerCase().endsWith(".md")) {
			readmeContent = md.render(mod.ReadmeContent);
		}
	}

	interface TreeNode {
		name: string;
		children?: TreeNode[];
		isFile: boolean;
		fullPath?: string;
	}

	function buildFileTree(files: string[], instancePath: string | undefined): TreeNode[] {
		const root: TreeNode = { name: "root", children: [], isFile: false };

		let normalizedBase = "";
		if (instancePath) {
			normalizedBase = instancePath.replaceAll("\\", "/");
			if (!normalizedBase.endsWith("/")) {
				normalizedBase += "/";
			}
		}

		for (const file of files) {
			let normalized = file.replaceAll("\\", "/");
			if (normalizedBase && normalized.startsWith(normalizedBase)) {
				normalized = normalized.slice(normalizedBase.length);
			}
			const parts = normalized.split("/").filter(Boolean);
			let current = root;

			for (let i = 0; i < parts.length; i++) {
				const part = parts[i];
				const isLast = i === parts.length - 1;

				if (!current.children) {
					current.children = [];
				}

				let found = current.children.find(
					(child) => child.name === part && child.isFile === isLast,
				);
				if (!found) {
					found = {
						name: part,
						isFile: isLast,
						fullPath: isLast ? file : undefined,
					};
					if (!isLast) {
						found.children = [];
					}
					current.children.push(found);
				}
				current = found;
			}
		}

		function sortTree(nodes: TreeNode[]): TreeNode[] {
			nodes.sort((a, b) => {
				if (a.isFile !== b.isFile) {
					return a.isFile ? 1 : -1;
				}
				return a.name.localeCompare(b.name);
			});
			for (const node of nodes) {
				if (node.children) {
					node.children = sortTree(node.children);
				}
			}
			return nodes;
		}

		return sortTree(root.children || []);
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<Header
		title={instance?.label || m.common_loading()}
		tabs={[{ name: m.instance_content(), value: "content" }]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
		iconBg={getInstanceColor(instance)}
	>
		{#snippet icon()}
			{#if isSettingUp}
				<span
					class="loading loading-spinner loading-md"
					style="color: {getContrastColor(getInstanceColor(instance))};"
				></span>
			{:else}
				<Box size={32} style="color: {getContrastColor(getInstanceColor(instance))};" />
			{/if}
		{/snippet}
		{#snippet actions()}
			<button
				class="btn text-sm"
				class:btn-accent={!isRunning}
				class:btn-error={isRunning}
				disabled={isLaunching || isKilling || isSettingUp}
				aria-busy={isLaunching || isKilling || isSettingUp}
				onclick={handleLaunchToggle}
			>
				{#if isLaunching}
					<span class="loading loading-spinner loading-xs"></span>
					{m.common_launching()}
				{:else if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
					{m.common_stopping_label()}
				{:else if isRunning}
					<Square size={16} />
					{m.common_stop()}
				{:else}
					<Play size={16} />
					{m.common_play()}
				{/if}
			</button>
			<button class="btn btn-square" onclick={() => (isSettingsModalOpen = true)}>
				<Settings size={16} />
			</button>
			{#if instance}
				<InstanceMenu {instance} bind:openSettingsModal={isSettingsModalOpen} />
			{/if}
		{/snippet}
		{#snippet subtitle()}
			<div class="flex items-center gap-1.5">
				<Gamepad2 size={14} />
				<span>{instance?.version || m.common_unknown_version()}</span>
				{#if instance?.launchOptions?.args && instance.launchOptions.args.length > 0}
					<div
						class="flex items-center gap-1 min-w-0 ml-1.5"
						title={instance.launchOptions.args.join(" ")}
					>
						<Terminal size={14} class="shrink-0" />
						<span class="text-xs font-mono truncate max-w-[350px]">
							{instance.launchOptions.args.join(" ")}
						</span>
					</div>
				{/if}
			</div>
		{/snippet}
		{#snippet errors()}
			{#if launchError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{launchError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearLaunchError}>
							{m.common_dismiss()}
						</button>
					</div>
				</div>
			{/if}
			{#if killError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{killError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearKillError}>
							{m.common_dismiss()}
						</button>
					</div>
				</div>
			{/if}
		{/snippet}
	</Header>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		{#if activeTab === "content"}
			<div class="flex-1 overflow-y-auto">
				<div class="px-4 py-6">
					{#if modsList.length === 0}
						<EmptyState
							title={m.instance_no_content()}
							description={m.instance_drag_import_hint()}
						>
							{#snippet icon()}
								<PackageOpen size={48} />
							{/snippet}
							{#snippet actions()}
								<button
									class="btn btn-accent btn-sm mt-2"
									onclick={handleInstallMod}
								>
									<PackageOpen size={14} />
									{m.instancemenu_install_mod()}
								</button>
							{/snippet}
						</EmptyState>
					{:else}
						<table class="table">
							<thead>
								<tr>
									<th>
										<button
											class="flex items-center gap-1 font-semibold text-sm"
										>
											<span>{m.common_name()}</span>
										</button>
									</th>
									<th class="w-48">{m.common_version()}</th>
									<th class="w-auto text-right">
										<button
											class="btn btn-ghost btn-sm"
											onclick={handleRefreshMods}
											disabled={isQueryLoading}
										>
											{#if isQueryLoading}
												<span class="loading loading-spinner loading-xs"
												></span>
											{:else}
												<RefreshCw size={14} />
											{/if}
											{m.common_refresh()}
										</button>
									</th>
								</tr>
							</thead>
							<tbody>
								{#each modsList as mod (mod.Id)}
									<tr class="hover">
										<td>
											<div class="flex items-center gap-3">
												<button
													class="w-10 h-10 rounded-lg bg-base-200 hover:bg-base-300 flex items-center justify-center shrink-0 transition-all text-primary hover:scale-105 active:scale-95 cursor-pointer"
													onclick={() => handleOpenFiles(mod)}
													title={m.mod_updated_files()}
												>
													<Box size={20} class="opacity-75" />
												</button>
												<div class="flex-1 min-w-0">
													<div class="flex items-center gap-2">
														<button
															class="font-bold text-sm truncate text-left hover:text-primary transition-colors cursor-pointer"
															onclick={() => handleOpenFiles(mod)}
															title={m.mod_updated_files()}
														>
															{mod.Name}
														</button>
													</div>
													{#if mod.Authors && mod.Authors.length > 0 && mod.Authors.some((a) => a.Link)}
														<button
															class="text-xs opacity-70 hover:opacity-100 cursor-pointer text-left block"
															onclick={() => handleOpenFiles(mod)}
														>
															by <span
																class="underline decoration-dashed decoration-1 underline-offset-2"
																>{mod.Author}</span
															>
														</button>
													{:else}
														<p class="text-xs opacity-70">
															by {mod.Author}
														</p>
													{/if}
												</div>
											</div>
										</td>
										<td>
											<p class="font-semibold text-sm">{mod.Version}</p>
										</td>
										<td>
											<div class="flex items-center justify-end gap-1">
												<button
													class="btn btn-error btn-sm btn-square"
													disabled={removeModMutation.isPending}
													onclick={() => handleRemoveMod(mod.Name)}
												>
													<Trash2 size={14} />
												</button>
												<ModMenu {mod} gamePath={instance?.path ?? ""} />
											</div>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</div>

<Modal
	bind:open={isFilesDialogOpen}
	title={selectedModForFiles?.Name || "Mod Details"}
	class="max-w-2xl"
>
	<div class="space-y-4">
		{#if selectedModForFiles}
			<div role="tablist" class="tabs tabs-box bg-base-200 p-1 rounded-box">
				<button
					role="tab"
					class="tab rounded-lg transition-all"
					class:tab-active={modDetailsTab === "details"}
					onclick={() => (modDetailsTab = "details")}
				>
					Details
				</button>
				<button
					role="tab"
					class="tab rounded-lg transition-all"
					class:tab-active={modDetailsTab === "readme"}
					onclick={() => (modDetailsTab = "readme")}
				>
					Readme
				</button>
				<button
					role="tab"
					class="tab rounded-lg transition-all"
					class:tab-active={modDetailsTab === "files"}
					onclick={() => (modDetailsTab = "files")}
				>
					Changed Files ({selectedModForFiles.InstalledFiles?.length ?? 0})
				</button>
			</div>

			<div class="h-[480px] overflow-hidden flex flex-col justify-start mt-4">
				{#if modDetailsTab === "details"}
					<div class="space-y-4 flex flex-col h-full overflow-hidden">
						<div class="stats bg-base-200 w-full shrink-0">
							<div class="stat">
								<div class="stat-title text-xs opacity-75">Version</div>
								<div class="stat-value text-sm font-mono font-semibold text-accent">
									{selectedModForFiles.Version}
								</div>
							</div>
							<div class="stat">
								<div class="stat-title text-xs opacity-75">Format / Kind</div>
								<div class="stat-value text-sm font-semibold">
									{selectedModForFiles.Kind}
								</div>
							</div>
						</div>

						<div
							class="flex flex-col gap-2 bg-base-200 p-4 rounded-box flex-1 overflow-hidden"
						>
							<span class="text-xs opacity-70 font-semibold uppercase tracking-wider"
								>Mod Authors</span
							>
							<div class="space-y-2 overflow-y-auto pr-1 flex-1">
								{#if selectedModForFiles.Authors && selectedModForFiles.Authors.length > 0}
									{#each selectedModForFiles.Authors as author}
										{#if author.Link}
											<a
												href={author.Link}
												target="_blank"
												rel="noopener noreferrer"
												class="flex flex-col gap-1 p-3 rounded-xl bg-base-200/50 hover:bg-base-300 transition-all cursor-pointer text-left block border border-transparent hover:border-accent/10"
											>
												<h4 class="font-bold text-sm truncate">
													{author.Name}
												</h4>
												<span
													class="text-xs text-accent hover:underline truncate block"
												>
													{author.Link}
												</span>
											</a>
										{:else}
											<div
												class="flex flex-col gap-1 p-3 rounded-xl bg-base-200/50"
											>
												<h4 class="font-bold text-sm truncate">
													{author.Name}
												</h4>
											</div>
										{/if}
									{/each}
								{:else}
									<div class="flex flex-col gap-1 p-3 rounded-xl bg-base-200/50">
										<h4 class="font-bold text-sm truncate">
											{selectedModForFiles.Author || "Unknown"}
										</h4>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{:else if modDetailsTab === "readme"}
					<div class="h-full overflow-y-auto overflow-x-hidden pr-3 break-words">
						{#if selectedModForFiles.ReadmeContent}
							{#if isReadmeMarkdown}
								<article
									class="prose prose-sm max-w-full text-base-content"
									style="--tw-prose-body: currentColor; --tw-prose-headings: currentColor; --tw-prose-bold: currentColor; --tw-prose-bullets: currentColor; --tw-prose-quotes: currentColor; --tw-prose-links: currentColor; --tw-prose-code: currentColor;"
								>
									{@html readmeContent}
								</article>
							{:else}
								<div
									class="whitespace-pre-wrap font-mono text-xs opacity-80 bg-base-200/40 p-5 rounded-xl border border-base-300 text-base-content"
								>
									{selectedModForFiles.ReadmeContent}
								</div>
							{/if}
						{:else}
							<p class="opacity-50 italic text-center py-12">
								No readme provided for this mod.
							</p>
						{/if}
					</div>
				{:else}
					<div class="h-full flex flex-col justify-start overflow-hidden">
						{#if !selectedModForFiles.InstalledFiles || selectedModForFiles.InstalledFiles.length === 0}
							<div
								class="bg-base-200/20 border border-dashed border-base-300 text-center py-8 text-base-content/60 h-full flex items-center justify-center rounded-box"
							>
								<p>{m.mod_no_files()}</p>
							</div>
						{:else}
							<div class="overflow-y-auto pr-1 bg-base-200 rounded-box p-3 h-full">
								<ul class="menu menu-xs p-0 font-mono w-full">
									{#each buildFileTree(selectedModForFiles.InstalledFiles, instance?.path) as node}
										{@render renderNode(node)}
									{/each}
								</ul>
							</div>
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	</div>
</Modal>

{#snippet renderNode(node: TreeNode)}
	{#if node.isFile}
		<li>
			<span class="flex items-center gap-2 py-1 select-none">
				<File size={14} class="text-primary shrink-0 opacity-80" />
				<span class="truncate" title={node.fullPath}>{node.name}</span>
			</span>
		</li>
	{:else}
		<li>
			<details open>
				<summary
					class="flex items-center gap-2 py-1 font-semibold select-none text-base-content/90"
				>
					<Folder size={14} class="shrink-0 opacity-80" />
					<span class="truncate">{node.name}</span>
				</summary>
				<ul class="before:bg-base-300 pl-4 ml-2">
					{#each node.children || [] as child}
						{@render renderNode(child)}
					{/each}
				</ul>
			</details>
		</li>
	{/if}
{/snippet}
