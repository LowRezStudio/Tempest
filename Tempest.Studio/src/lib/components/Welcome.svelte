<script lang="ts">
	import { AppWindow, Clock, FileUp, FolderOpen, Hammer, PackageOpen, X } from "@lucide/svelte";
	import { onMount } from "svelte";
	import { invoke, pickDirectory, pickFile, PACKAGE_FILTERS } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	let backend = $state<{ running: boolean; binary: string | null; error: string | null } | null>(
		null,
	);

	onMount(async () => {
		backend = await invoke("upk:status");
	});

	async function browse() {
		const dir = await pickDirectory("Open a cooked folder (CookedPCConsole)");
		if (dir) await studio.openWorkspace(dir);
	}

	async function openFile() {
		const f = await pickFile("Open a package", PACKAGE_FILTERS);
		if (f) await studio.openFiles([f]);
	}

	async function restart() {
		backend = await invoke("upk:restart");
	}
</script>

<div class="flex h-full items-center justify-center overflow-y-auto p-6">
	<div class="w-[min(36rem,100%)] space-y-6">
		<div class="flex items-center gap-3">
			<img src="/studio-icon.png" alt="" class="h-12 w-12" />
			<div>
				<h1 class="text-2xl font-bold">Tempest Studio</h1>
				<p class="text-sm opacity-70">
					Make Paladins mods: edit packages and the game data, build a mod.
				</p>
			</div>
		</div>

		{#if backend && !backend.running}
			<div class="alert alert-error text-sm">
				<div>
					<div class="font-semibold">The upk backend is not running</div>
					<div class="selectable">{backend.error ?? "unknown error"}</div>
					<div class="mt-1 opacity-80">
						Build it with <code>cargo build --release</code> in Tempest.UnrealScript, then
						retry.
					</div>
				</div>
				<button class="btn btn-sm" onclick={restart}>Retry</button>
			</div>
		{/if}

		<div class="grid gap-3 sm:grid-cols-2">
			<button
				class="btn btn-primary h-auto flex-col items-start gap-1 py-3 text-left normal-case"
				onclick={() => ui.open("newProject")}
				disabled={studio.busy !== null}
			>
				<span class="flex items-center gap-2 text-base"
					><Hammer size={18} /> New mod project</span
				>
				<span class="text-xs font-normal opacity-80"
					>Name your mod, pick the game folder; your edits are kept as you work.</span
				>
			</button>
			<button
				class="btn h-auto flex-col items-start gap-1 py-3 text-left normal-case"
				onclick={() => session.pickAndOpen()}
				disabled={studio.busy !== null}
			>
				<span class="flex items-center gap-2 text-base"
					><PackageOpen size={18} /> Open a project</span
				>
				<span class="text-xs font-normal opacity-70">Continue a mod (a .twrk file).</span>
			</button>
		</div>
		<div class="flex flex-wrap items-center gap-2 text-xs">
			<span class="opacity-60">Or just look around:</span>
			<button class="btn btn-ghost btn-xs" onclick={browse} disabled={studio.busy !== null}>
				<FolderOpen size={13} /> Open a game folder…
			</button>
			<button class="btn btn-ghost btn-xs" onclick={openFile} disabled={studio.busy !== null}>
				<FileUp size={13} /> Open a package file…
			</button>
		</div>
		{#if studio.pendingFiles.length > 0}
			<div class="alert alert-info text-xs">
				<span
					>{studio.pendingFiles.map((f) => f.split(/[/\\]/).at(-1)).join(", ")} will open once
					a game folder is chosen (it provides the class database).</span
				>
			</div>
		{/if}

		{#if session.recents.length > 0}
			<div>
				<div
					class="mb-1 flex items-center gap-1 text-xs font-semibold uppercase opacity-60"
				>
					<PackageOpen size={12} /> Recent projects
					<span class="flex-1"></span>
					<button
						class="btn btn-ghost btn-xs font-normal normal-case"
						onclick={() => session.forgetRecent()}
						title="Clear the list (the project files stay where they are)">Clear</button
					>
				</div>
				<ul class="menu rounded-box bg-base-200 w-full overflow-hidden p-1">
					{#each session.recents as file (file)}
						<li class="min-w-0 flex-row flex-nowrap items-center">
							<button
								class="min-w-0 flex-1 flex-col items-start gap-0"
								onclick={() => session.open(file)}
								disabled={studio.busy !== null}
								title={file}
							>
								<span class="font-semibold"
									>{basename(file).replace(/\.twrk$/i, "")}</span
								>
								<span class="max-w-full truncate font-mono text-[11px] opacity-60"
									>{file}</span
								>
							</button>
							<button
								class="btn btn-ghost btn-xs btn-square"
								onclick={() => session.forgetRecent(file)}
								title="Remove from the list (the project stays where it is)"
								aria-label="Remove from recent projects"
							>
								<X size={12} />
							</button>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if studio.recents.length > 0}
			<div>
				<div
					class="mb-1 flex items-center gap-1 text-xs font-semibold uppercase opacity-60"
				>
					<Clock size={12} /> Recent game folders
					<span class="flex-1"></span>
					<button
						class="btn btn-ghost btn-xs font-normal normal-case"
						onclick={() => studio.forgetRecent()}
						title="Clear the list (the folders stay where they are)">Clear</button
					>
				</div>
				<ul class="menu rounded-box bg-base-200 w-full overflow-hidden p-1">
					{#each studio.recents as dir (dir)}
						<li class="min-w-0 flex-row flex-nowrap items-center">
							<button
								class="min-w-0 flex-1 truncate font-mono text-xs"
								title={dir}
								onclick={() => studio.openWorkspace(dir)}
								disabled={studio.busy !== null}
							>
								{dir}
							</button>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => studio.newWindow(dir)}
								title="Open in a new window"
							>
								<AppWindow size={12} />
							</button>
							<button
								class="btn btn-ghost btn-xs btn-square"
								onclick={() => studio.forgetRecent(dir)}
								title="Remove from the list (the folder stays where it is)"
								aria-label="Remove from recent game folders"
							>
								<X size={12} />
							</button>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if studio.busy}
			<div class="flex items-center gap-2 text-sm">
				<span class="loading loading-sm loading-spinner"></span>
				{studio.busy}
			</div>
		{/if}
	</div>
</div>
