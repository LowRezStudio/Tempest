<script lang="ts">
	import { Tabs } from "bits-ui";
	import { marked } from "marked";
	import ModFileTree from "$lib/components/mods/ModFileTree.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { ModRecord } from "$lib/core/mods";

	interface Props {
		mod: ModRecord | null;
		open?: boolean;
		instancePath?: string;
	}

	let { mod, open = $bindable(false), instancePath }: Props = $props();

	let tab = $state<"details" | "readme" | "files">("details");

	$effect(() => {
		if (open) tab = "details";
	});

	let isReadmeMarkdown = $derived(mod?.Readme ? mod.Readme.toLowerCase().endsWith(".md") : false);

	let readmeContent = $derived(
		mod?.ReadmeContent && isReadmeMarkdown
			? (marked.parse(mod.ReadmeContent, { async: false }) as string)
			: "",
	);
</script>

<Modal bind:open title={mod?.Name || "Mod Details"} class="max-w-2xl">
	<div class="space-y-4">
		{#if mod}
			<Tabs.Root bind:value={tab}>
				<Tabs.List class="tabs tabs-box bg-base-200 rounded-box p-1">
					<Tabs.Trigger
						value="details"
						class="tab data-[state=active]:tab-active rounded-lg transition-all"
					>
						Details
					</Tabs.Trigger>
					<Tabs.Trigger
						value="readme"
						class="tab data-[state=active]:tab-active rounded-lg transition-all"
					>
						Readme
					</Tabs.Trigger>
					<Tabs.Trigger
						value="files"
						class="tab data-[state=active]:tab-active rounded-lg transition-all"
					>
						Changed Files ({mod.InstalledFiles?.length ?? 0})
					</Tabs.Trigger>
				</Tabs.List>
			</Tabs.Root>

			<div class="mt-4 flex h-[480px] flex-col justify-start overflow-hidden">
				{#if tab === "details"}
					<div class="flex h-full flex-col space-y-4 overflow-hidden">
						<div class="stats bg-base-200 w-full shrink-0">
							<div class="stat">
								<div class="stat-title text-xs opacity-75">Version</div>
								<div class="stat-value text-accent font-mono text-sm font-semibold">
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
							class="bg-base-200 rounded-box flex flex-1 flex-col gap-2 overflow-hidden p-4"
						>
							<span class="text-xs font-semibold tracking-wider uppercase opacity-70"
								>Mod Authors</span
							>
							<div class="flex-1 space-y-2 overflow-y-auto pr-1">
								{#if mod.Authors && mod.Authors.length > 0}
									{#each mod.Authors as author}
										{#if author.Link}
											<a
												href={author.Link}
												target="_blank"
												rel="noopener noreferrer"
												class="bg-base-200/50 hover:bg-base-300 hover:border-accent/10 block flex cursor-pointer flex-col gap-1 rounded-xl border border-transparent p-3 text-left transition-all"
											>
												<h4 class="truncate text-sm font-bold">
													{author.Name}
												</h4>
												<span
													class="text-accent block truncate text-xs hover:underline"
												>
													{author.Link}
												</span>
											</a>
										{:else}
											<div
												class="bg-base-200/50 flex flex-col gap-1 rounded-xl p-3"
											>
												<h4 class="truncate text-sm font-bold">
													{author.Name}
												</h4>
											</div>
										{/if}
									{/each}
								{:else}
									<div class="bg-base-200/50 flex flex-col gap-1 rounded-xl p-3">
										<h4 class="truncate text-sm font-bold">
											{mod.Author || "Unknown"}
										</h4>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{:else if tab === "readme"}
					<div
						class="bg-base-200 rounded-box h-full overflow-x-hidden overflow-y-auto p-3 pr-3 break-words"
					>
						{#if mod.ReadmeContent}
							{#if isReadmeMarkdown}
								<article
									class="prose prose-sm text-base-content max-w-full"
									style="--tw-prose-body: currentColor; --tw-prose-headings: currentColor; --tw-prose-bold: currentColor; --tw-prose-bullets: currentColor; --tw-prose-quotes: currentColor; --tw-prose-links: currentColor; --tw-prose-code: currentColor;"
								>
									{@html readmeContent}
								</article>
							{:else}
								<div
									class="bg-base-200/40 border-base-300 text-base-content rounded-xl border p-5 font-mono text-xs whitespace-pre-wrap opacity-80"
								>
									{mod.ReadmeContent}
								</div>
							{/if}
						{:else}
							<p class="py-12 text-center italic opacity-50">
								No readme provided for this mod.
							</p>
						{/if}
					</div>
				{:else if tab === "files"}
					<div class="flex h-full flex-col justify-start overflow-hidden">
						{#if !mod.InstalledFiles || mod.InstalledFiles.length === 0}
							<div
								class="bg-base-200/20 border-base-300 text-base-content/60 rounded-box flex h-full items-center justify-center border border-dashed py-8 text-center"
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
