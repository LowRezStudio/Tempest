<script lang="ts">
	import { Tabs } from "bits-ui";
	import ModFileTree from "$lib/components/mods/ModFileTree.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { marked } from "marked";
	import type { ModRecord } from "$lib/core/mods";

	interface Props {
		mod: ModRecord | null;
		open?: boolean;
		instancePath?: string;
	}

	let { mod, open = $bindable(false), instancePath }: Props = $props();

	let tab = $state<"details" | "readme" | "files">("details");

	// Reset to the details tab whenever the modal opens.
	$effect(() => {
		if (open) {
			tab = "details";
		}
	});

	let isReadmeMarkdown = $derived(mod?.Readme ? mod.Readme.toLowerCase().endsWith(".md") : false);

	let readmeContent = $derived(
		mod?.ReadmeContent && isReadmeMarkdown ? (marked.parse(mod.ReadmeContent, { async: false }) as string) : "",
	);
</script>

<Modal bind:open title={mod?.Name || "Mod Details"} class="max-w-2xl">
	<div class="space-y-4">
		{#if mod}
			<Tabs.Root bind:value={tab}>
				<Tabs.List class="tabs tabs-box bg-base-200 p-1 rounded-box">
					<Tabs.Trigger
						value="details"
						class="tab rounded-lg transition-all data-[state=active]:tab-active"
					>
						Details
					</Tabs.Trigger>
					<Tabs.Trigger
						value="readme"
						class="tab rounded-lg transition-all data-[state=active]:tab-active"
					>
						Readme
					</Tabs.Trigger>
					<Tabs.Trigger
						value="files"
						class="tab rounded-lg transition-all data-[state=active]:tab-active"
					>
						Changed Files ({mod.InstalledFiles?.length ?? 0})
					</Tabs.Trigger>
				</Tabs.List>
			</Tabs.Root>

			<div class="h-[480px] overflow-hidden flex flex-col justify-start mt-4">
				{#if tab === "details"}
					<div class="space-y-4 flex flex-col h-full overflow-hidden">
						<div class="stats bg-base-200 w-full shrink-0">
							<div class="stat">
								<div class="stat-title text-xs opacity-75">Version</div>
								<div class="stat-value text-sm font-mono font-semibold text-accent">
									{mod.Version}
								</div>
							</div>
							<div class="stat">
								<div class="stat-title text-xs opacity-75">Format / Kind</div>
								<div class="stat-value text-sm font-semibold">
									{mod.Kind}
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
								{#if mod.Authors && mod.Authors.length > 0}
									{#each mod.Authors as author}
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
											{mod.Author || "Unknown"}
										</h4>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{:else if tab === "readme"}
					<div class="h-full overflow-y-auto overflow-x-hidden pr-3 break-words">
						{#if mod.ReadmeContent}
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
									{mod.ReadmeContent}
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
						{#if !mod.InstalledFiles || mod.InstalledFiles.length === 0}
							<div
								class="bg-base-200/20 border border-dashed border-base-300 text-center py-8 text-base-content/60 h-full flex items-center justify-center rounded-box"
							>
								<p>{m.mod_no_files()}</p>
							</div>
						{:else}
							<ModFileTree files={mod.InstalledFiles} basePath={instancePath} />
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	</div>
</Modal>
