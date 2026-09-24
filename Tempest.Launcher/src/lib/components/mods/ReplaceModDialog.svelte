<script lang="ts">
	import { AlertTriangle, Box, File, Layers, Replace, Swords, PlusCircle } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	type Choice = "cancel" | "replace" | "stack";

	interface Props {
		open: boolean;
		modName: string;
		newModName?: string;
		conflictingMods?: Array<{
			ModId: string;
			ModName: string;
			ModVersion?: string;
			ConflictingFiles: string[];
		}>;
		onconfirm: (choice: Choice) => void;
		oncancel: () => void;
	}

	let {
		open = $bindable(false),
		modName,
		newModName,
		conflictingMods = [],
		onconfirm,
		oncancel,
	}: Props = $props();

	let displayNew = $derived(newModName || modName);
	let hasDetailedConflicts = $derived(conflictingMods.length > 0);
	let uniqueFiles = $derived([...new Set(conflictingMods.flatMap((c) => c.ConflictingFiles))]);
	let fileCount = $derived(uniqueFiles.length);
	let expanded = $state(true);

	function handleConfirm(choice: Choice) {
		onconfirm(choice);
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}

	function fileIcon(path: string) {
		const ext = path.split(".").pop()?.toLowerCase() ?? "";
		if (ext === "upk" || ext === "pck") return "upk";
		if (ext === "ini") return "ini";
		if (ext === "dll") return "dll";
		return "file";
	}
</script>

<Modal bind:open class="max-w-xl" onclose={handleCancel}>
	<div class="space-y-5">
		{#if hasDetailedConflicts}
			<!-- Minimal header — left aligned, enlarged -->
			<div class="flex items-center gap-3 pt-0">
				<div
					class="bg-warning/10 border-warning/20 flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border"
				>
					<Swords size={18} class="text-warning" />
				</div>
				<div class="text-left">
					<h4 class="text-base leading-none font-bold tracking-wide uppercase">
						{m.conflict_header_title()}
					</h4>
					<p class="text-sm opacity-60">{m.conflict_header_subtitle()}</p>
				</div>
			</div>

			<!-- Tiles — blue left, red right, no VS -->
			<div class="grid grid-cols-2 items-stretch gap-2">
				<!-- Incoming — blue -->
				<div
					class="border-primary/20 bg-primary/5 flex flex-col gap-2.5 rounded-xl border p-3"
				>
					<div class="flex items-center gap-2">
						<div
							class="bg-primary text-primary-content flex h-7 w-7 shrink-0 items-center justify-center rounded-lg"
						>
							<Box size={14} />
						</div>
						<span
							class="badge badge-primary badge-xs px-1.5 text-[10px] font-bold tracking-widest"
							>{m.conflict_badge_new()}</span
						>
					</div>
					<div class="min-w-0">
						<p class="truncate text-sm leading-tight font-semibold" title={displayNew}>
							{displayNew}
						</p>
						<p class="truncate text-[11px] opacity-60">
							{m.conflict_will_be_installed({ count: fileCount })}
						</p>
					</div>
				</div>

				<!-- Installed — red, same elements as left -->
				<div class="border-error/20 bg-error/5 flex flex-col gap-2.5 rounded-xl border p-3">
					<div class="flex items-center gap-2">
						<div
							class="bg-error text-error-content flex h-7 w-7 shrink-0 items-center justify-center rounded-lg"
						>
							<Box size={14} />
						</div>
						<span
							class="badge badge-error badge-xs px-1.5 text-[10px] font-bold tracking-widest"
							>{m.conflict_badge_installed()}</span
						>
					</div>
					<div class="min-w-0">
						{#if conflictingMods.length === 1}
							<p
								class="truncate text-sm leading-tight font-semibold"
								title={conflictingMods[0].ModName}
							>
								{conflictingMods[0].ModName}
							</p>
							<p class="truncate text-[11px] opacity-60">
								{m.conflict_will_be_removed({ count: fileCount })}
							</p>
						{:else}
							<p class="truncate text-sm leading-tight font-semibold">
								{m.conflict_mods_count({ count: conflictingMods.length })}
							</p>
							<p class="truncate text-[11px] opacity-60">
								{conflictingMods
									.map((c) => c.ModName)
									.slice(0, 2)
									.join(", ")}{conflictingMods.length > 2
									? ` +${conflictingMods.length - 2}`
									: ""}
							</p>
						{/if}
					</div>
				</div>
			</div>

			{#if conflictingMods.length > 1}
				<div class="grid gap-2">
					{#each conflictingMods as cm (cm.ModId)}
						<div
							class="bg-base-200 border-base-300 hover:bg-base-300/60 flex items-center gap-3 rounded-xl border px-3 py-2.5 transition-colors"
						>
							<div
								class="bg-base-100 border-base-300 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border"
							>
								<Layers size={14} class="opacity-60" />
							</div>
							<div class="min-w-0 flex-1">
								<p class="truncate text-sm leading-none font-semibold">
									{cm.ModName}
								</p>
								<p class="truncate text-xs opacity-50">
									{m.conflict_row_overlapping_files({
										count: cm.ConflictingFiles.length,
									})}
								</p>
							</div>
							<span class="badge badge-ghost badge-sm font-mono text-[11px]"
								>{cm.ModVersion ? `v${cm.ModVersion}` : "installed"}</span
							>
						</div>
					{/each}
				</div>
			{/if}

			<!-- Overlapping files - interactive -->
			<div class="border-base-300 bg-base-200/70 overflow-hidden rounded-2xl border">
				<button
					type="button"
					class="hover:bg-base-300/50 flex w-full items-center gap-3 px-4 py-3 text-left transition-colors"
					onclick={() => (expanded = !expanded)}
				>
					<div
						class="bg-warning/15 text-warning border-warning/20 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border"
					>
						<File size={14} />
					</div>
					<div class="min-w-0 flex-1">
						<p class="text-sm leading-none font-bold">
							{m.conflict_overlapping_components({ count: fileCount })}
						</p>
						<p class="text-xs opacity-60">
							{expanded ? m.conflict_tap_hide() : m.conflict_tap_inspect()}
						</p>
					</div>
					<div class="flex shrink-0 items-center gap-2">
						<span class="badge badge-warning badge-sm px-2 font-mono text-xs"
							>{fileCount}</span
						>
						<span
							class="text-base-content/30 transition-transform {expanded
								? 'rotate-180'
								: ''}">▾</span
						>
					</div>
				</button>

				{#if expanded}
					<div class="px-3 pb-3">
						<div class="custom-scrollbar grid max-h-40 gap-1.5 overflow-auto pr-1">
							{#each uniqueFiles as f (f)}
								<div
									class="group bg-base-100 border-base-300 hover:border-warning/30 hover:bg-warning/5 flex cursor-default items-center gap-2.5 rounded-xl border px-3 py-2 transition-all"
									title={f}
								>
									<div
										class="bg-base-200 group-hover:bg-warning/10 border-base-300 group-hover:border-warning/20 flex h-6 w-6 shrink-0 items-center justify-center rounded-lg border transition-colors"
									>
										<File
											size={12}
											class="group-hover:text-warning opacity-60 transition-colors group-hover:opacity-100"
										/>
									</div>
									<span class="flex-1 truncate font-mono text-xs">{f}</span>
									<span
										class="badge badge-ghost badge-xs hidden font-mono text-[10px] opacity-50 group-hover:opacity-100 sm:inline-flex"
									>
										{fileIcon(f)}
									</span>
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		{:else}
			<div class="flex items-start gap-3">
				<div class="text-warning mt-0.5 shrink-0">
					<AlertTriangle size={24} />
				</div>
				<div>
					<h4 class="text-base font-bold">
						{m.conflict_replace_mod_heading()}
					</h4>
					<p class="mt-1 text-sm opacity-70">
						{m.conflict_mod_message({ name: modName })}
					</p>
				</div>
			</div>
			<p class="text-sm opacity-60">
				{m.conflict_overwrite_warning()}
			</p>
		{/if}
	</div>

	{#snippet actions()}
		<button class="btn btn-primary" type="button" onclick={() => handleConfirm("stack")}>
			<PlusCircle size={16} />
			{m.conflict_stack_mod_btn()}
		</button>
		<button
			class="btn btn-error shadow-md transition-shadow hover:shadow-lg"
			type="button"
			onclick={() => handleConfirm("replace")}
		>
			<Replace size={16} />
			{m.conflict_replace_mod_btn()}
		</button>
	{/snippet}
</Modal>

<style>
	.custom-scrollbar::-webkit-scrollbar {
		width: 6px;
	}
	.custom-scrollbar::-webkit-scrollbar-thumb {
		background: color-mix(in oklab, var(--color-base-300) 80%, transparent);
		border-radius: 999px;
	}
</style>
