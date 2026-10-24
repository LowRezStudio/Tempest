<script lang="ts">
	import {
		CircleAlert,
		CloudDownload,
		FolderOpen,
		FolderX,
		Gamepad2,
		RotateCw,
		ShoppingBag,
		UserRound,
		X,
	} from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { open as openDialog } from "$lib/platform/api";
	import { defaultInstancePath } from "$lib/stores/settings.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { findExistingInstance, type FolderImport } from "./folder-import.svelte";
	import LoginMethodPicker from "./LoginMethodPicker.svelte";
	import type { InstallationSource } from "$lib/platforms/discovery";

	interface Props {
		importer: FolderImport;
	}

	let { importer }: Props = $props();

	const sources: Record<InstallationSource, { Icon: typeof X; label?: () => string }> = {
		steam: { Icon: Gamepad2, label: m.wizard_login_steam },
		epic: { Icon: ShoppingBag, label: m.wizard_login_epic },
		hirez: { Icon: UserRound, label: m.wizard_login_hirez },
		tempest: { Icon: CloudDownload, label: m.wizard_source_tempest },
		folder: { Icon: FolderOpen },
	};

	let pathInput = $state("");

	async function browseGameFolders() {
		const result = await openDialog({
			directory: true,
			multiple: true,
			title: m.wizard_select_installation_folder(),
			defaultPath: defaultInstancePath.value || undefined,
		});
		importer.add((result ?? []).filter(Boolean).map((path) => ({ path })));
	}

	function commitPathInput() {
		const path = pathInput.trim();
		if (path) importer.add([{ path }]);
		pathInput = "";
	}
</script>

<div class="flex flex-col gap-1">
	<div class="flex min-h-6 items-center justify-between gap-2">
		<span class="text-base-content/60 text-xs">{m.wizard_found_title()}</span>
		{#if importer.found.isFetching}
			<span class="text-base-content/60 flex items-center gap-1.5 text-xs">
				<span class="loading loading-spinner loading-xs"></span>
				{m.wizard_scanning()}
			</span>
		{:else}
			<button
				type="button"
				class="btn btn-ghost btn-xs"
				onclick={() => importer.found.refetch()}
			>
				<RotateCw size={12} />
				{m.wizard_scan_again()}
			</button>
		{/if}
	</div>

	{#if importer.folders.length > 0}
		<ul
			class="bg-base-100 border-base-300 rounded-field divide-base-300 max-h-64 divide-y overflow-y-auto border"
		>
			{#each importer.listed as folder (folder.key)}
				{@const existing = findExistingInstance(folder.path)}
				{@const found = importer.patchOf(folder)}
				{@const color = getInstanceColor(
					existing ?? { label: importer.label(folder, found) },
				)}
				{@const contrast = getContrastColor(color)}
				{@const source = folder.source ? sources[folder.source] : undefined}
				{@const usable = !folder.error && !existing}
				<li>
					<label
						class={[
							"flex items-center gap-3 p-2.5 transition-colors",
							usable ? "hover:bg-base-200/60 cursor-pointer" : "cursor-default",
							usable && folder.selected && "bg-primary/5",
							existing && "opacity-60",
						]}
					>
						<input
							type="checkbox"
							class="checkbox checkbox-sm checkbox-primary shrink-0"
							checked={usable && folder.selected}
							disabled={!usable}
							aria-label={folder.path}
							onchange={(e) => (folder.selected = e.currentTarget.checked)}
						/>
						{#if folder.loading}
							<span
								class="bg-base-300 flex size-8 shrink-0 items-center justify-center rounded-full"
							>
								<span class="loading loading-spinner loading-xs"></span>
							</span>
						{:else if folder.error}
							<span
								class="bg-error/15 text-error flex size-8 shrink-0 items-center justify-center rounded-full"
							>
								<FolderX size={16} />
							</span>
						{:else if found}
							<!-- The sidebar's button the instance will get -->
							<span
								class="rounded-field flex size-8 shrink-0 items-center justify-center text-[10px] font-bold tabular-nums"
								style="background-color: {color}; color: {contrast};"
								>{found.version}</span
							>
						{:else}
							<span
								class="bg-warning/15 text-warning flex size-8 shrink-0 items-center justify-center rounded-full"
							>
								<CircleAlert size={16} />
							</span>
						{/if}
						<span class="min-w-0 flex-1">
							<span class="block truncate font-mono text-xs" title={folder.path}>
								{folder.path}
							</span>
							{#if folder.error}
								<span class="text-error block text-xs">{folder.error}</span>
							{:else}
								<span
									class="text-base-content/60 flex items-center gap-1 truncate text-xs"
								>
									{#if source?.label}
										<source.Icon size={12} class="shrink-0" />
										{source.label()} ·
									{/if}
									{#if existing}
										{m.wizard_already_in_library({ name: existing.label })}
									{:else if folder.loading}
										{m.common_identifying()}
									{:else}
										{#if found}
											<span class="text-base-content/80">{found.name}</span>
											·
										{/if}
										{#if folder.modsLoading}
											{m.wizard_finding_mods()}
										{:else}
											<span
												title={folder.mods
													.map((mod) => mod.Name)
													.join(", ")}
											>
												{m.library_mods({ count: folder.mods.length })}
											</span>
										{/if}
									{/if}
								</span>
							{/if}
						</span>
						{#if !folder.source}
							<button
								type="button"
								class="btn btn-ghost btn-xs btn-square shrink-0"
								aria-label={m.wizard_remove_folder()}
								title={m.wizard_remove_folder()}
								onclick={() => importer.remove(folder.key)}
							>
								<X size={14} />
							</button>
						{/if}
					</label>
				</li>
			{/each}
		</ul>
	{:else if importer.found.isFetching}
		<div
			class="border-base-300 rounded-field loading-shimmer text-base-content/60 border px-3 py-4 text-center text-xs"
		>
			{m.wizard_found_scanning()}
		</div>
	{:else if importer.found.isError}
		<div
			class="bg-error/10 text-error rounded-field px-3 py-3 text-center text-xs"
			role="alert"
		>
			{m.wizard_found_failed()}
			<span class="mt-0.5 block font-mono wrap-break-word opacity-70"
				>{importer.found.error.message}</span
			>
		</div>
	{:else}
		<div
			class="border-base-300 rounded-field text-base-content/60 border border-dashed px-3 py-4 text-center text-xs"
		>
			{m.wizard_found_none()}
		</div>
	{/if}
</div>

<div class="flex flex-col gap-1">
	<div class="join w-full">
		<input
			type="text"
			class="input input-sm join-item min-w-0 flex-1 font-mono"
			spellcheck="false"
			aria-label={m.wizard_game_folder()}
			placeholder={m.wizard_add_folder_placeholder()}
			bind:value={pathInput}
			onchange={commitPathInput}
			onkeydown={(e) => {
				// Enter adds the path instead of submitting the form.
				if (e.key === "Enter") {
					e.preventDefault();
					commitPathInput();
				}
			}}
		/>
		<button type="button" class="btn btn-sm join-item" onclick={browseGameFolders}>
			<FolderOpen size={14} />
			{m.common_browse()}
		</button>
	</div>
	<p class="text-base-content/60 text-xs">{m.wizard_game_folder_hint()}</p>
</div>

{#if importer.needsLogin}
	<LoginMethodPicker bind:value={importer.loginMethod} />
{/if}
