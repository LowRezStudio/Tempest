<script lang="ts">
	import { ChevronDown, ChevronRight, File, FileText, FileX, Folder } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";

	interface Props {
		files: string[];
		basePath?: string;
		modId?: string;
		ownedFiles?: string[];
	}

	let { files, basePath, modId, ownedFiles = [] }: Props = $props();

	function normalizePath(path: string): string {
		return path.replaceAll("\\", "/");
	}

	interface TreeNode {
		name: string;
		children?: TreeNode[];
		isFile: boolean;
		fullPath?: string;
		transferred?: boolean;
	}

	function buildFileTree(files: string[], instancePath: string | undefined): TreeNode[] {
		const root: TreeNode = { name: "root", children: [], isFile: false };

		// Normalize ownedFiles for comparison
		const ownedSet = new Set(ownedFiles.map((f) => normalizePath(f).toLowerCase()));

		let normalizedBase = "";
		if (instancePath) {
			normalizedBase = normalizePath(instancePath);
			if (!normalizedBase.endsWith("/")) {
				normalizedBase += "/";
			}
		}

		for (const file of files) {
			let normalized = normalizePath(file);
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
					const fullPath = isLast ? file : undefined;
					// INI files use merge/patch system, not ownership transfer - never mark as transferred
					const isIni = isLast && fullPath?.toLowerCase().endsWith(".ini");
					// Check if this file is in ownedFiles (normalize path for comparison)
					const normalizedFullPath = normalizePath(fullPath ?? "").toLowerCase();
					const isTransferred =
						isLast && !isIni ? !ownedSet.has(normalizedFullPath) : false;
					found = {
						name: part,
						isFile: isLast,
						fullPath,
						transferred: isTransferred,
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

	let tree = $derived(buildFileTree(files, basePath));

	const TEXT_PREVIEW_EXTS = new Set([
		".txt",
		".ini",
		".cfg",
		".log",
		".json",
		".toml",
		".yaml",
		".yml",
		".xml",
		".csv",
	]);

	function isTextPreviewable(name: string): boolean {
		const lower = name.toLowerCase();
		for (const ext of TEXT_PREVIEW_EXTS) if (lower.endsWith(ext)) return true;
		return false;
	}

	// keep old name as alias for backwards compat
	const isTxtFile = isTextPreviewable;

	function getExtLabel(name: string): string {
		const lower = name.toLowerCase();
		for (const ext of TEXT_PREVIEW_EXTS) if (lower.endsWith(ext)) return ext;
		return "";
	}

	let expandedPaths = $state<Set<string>>(new Set());
	let fileContents = $state<Map<string, { loading: boolean; text: string; error: string }>>(
		new Map(),
	);

	async function toggleTxt(path: string, name: string) {
		if (!isTextPreviewable(name)) return;
		const key = path;
		if (expandedPaths.has(key)) {
			expandedPaths.delete(key);
			expandedPaths = new Set(expandedPaths);
			return;
		}
		expandedPaths.add(key);
		expandedPaths = new Set(expandedPaths);

		if (fileContents.has(key)) return;

		fileContents.set(key, { loading: true, text: "", error: "" });
		fileContents = new Map(fileContents);
		try {
			// Path resolution (.ini mod fragments, relative paths) and the read
			// happen host-side, sandboxed to the instance directory.
			const { text: raw, truncated } = await invoke("mods:read-file", {
				basePath: basePath ?? "",
				modId,
				path: key,
			});
			// Keep the UI responsive on very large files (host caps at 50k chars).
			const text = truncated ? `${raw}${m.mod_details_file_truncated()}` : raw;
			fileContents.set(key, { loading: false, text, error: "" });
		} catch (error) {
			fileContents.set(key, {
				loading: false,
				text: "",
				error: error instanceof Error ? error.message : String(error),
			});
		}
		fileContents = new Map(fileContents);
	}
</script>

{#snippet renderNode(node: TreeNode)}
	{#if node.isFile}
		{@const txt = isTextPreviewable(node.name)}
		{@const key = node.fullPath ?? node.name}
		{@const isExpanded = expandedPaths.has(key)}
		{@const state = fileContents.get(key)}
		{@const isTransferred = node.transferred === true}
		<li class={isTransferred ? "line-through opacity-40" : ""}>
			{#if txt}
				<button
					type="button"
					class="hover:bg-base-300 flex w-full items-center gap-2 rounded px-1 py-1 text-left select-none"
					title={node.fullPath}
					onclick={() => toggleTxt(key, node.name)}
				>
					{#if isExpanded}
						<ChevronDown size={12} class="shrink-0 opacity-60" />
					{:else}
						<ChevronRight size={12} class="shrink-0 opacity-60" />
					{/if}
					<FileText size={14} class="text-primary shrink-0 opacity-80" />
					<span class="truncate">{node.name}</span>
					<span class="ml-auto shrink-0 text-[10px] opacity-40"
						>{getExtLabel(node.name)}</span
					>
				</button>
				{#if isExpanded}
					<div class="mt-1 mb-1 ml-6">
						{#if state?.loading}
							<div
								class="bg-base-300/50 border-base-300 flex items-center gap-2 rounded border px-3 py-2 text-xs opacity-60"
							>
								<span class="loading loading-spinner loading-xs"></span>
								{m.common_loading()}
							</div>
						{:else if state?.error}
							<div
								class="bg-error/10 text-error border-error/20 rounded border px-3 py-2 font-mono text-xs"
							>
								{m.mod_details_file_read_failed({ error: state.error })}
							</div>
						{:else if state?.text !== undefined}
							{#if state.text.length === 0}
								<div
									class="bg-base-300/30 border-base-300 rounded border px-3 py-2 text-xs italic opacity-50"
								>
									{m.mod_details_file_empty()}
								</div>
							{:else}
								<pre
									class="bg-base-300/70 border-base-300 text-base-content max-h-64 overflow-auto rounded border px-3 py-2 font-mono text-xs break-words whitespace-pre-wrap select-text">{state.text}</pre>
							{/if}
						{/if}
					</div>
				{/if}
			{:else}
				<span class="flex items-center gap-2 px-1 py-1 select-none">
					<File size={14} class="text-primary shrink-0 opacity-80" />
					<span class="truncate" title={node.fullPath}>{node.name}</span>
				</span>
			{/if}
		</li>
	{:else}
		<li>
			<details open>
				<summary
					class="text-base-content/90 flex items-center gap-2 rounded px-1 py-1 font-semibold select-none"
				>
					<Folder size={14} class="shrink-0 opacity-80" />
					<span class="truncate">{node.name}</span>
				</summary>
				<ul class="before:bg-base-300 ml-2 pl-4">
					{#each node.children || [] as child}
						{@render renderNode(child)}
					{/each}
				</ul>
			</details>
		</li>
	{/if}
{/snippet}

{#if tree.length === 0}
	<div
		class="text-base-content/50 flex h-full flex-col items-center justify-center gap-2 py-12 text-center text-sm"
	>
		<FileX size={28} class="opacity-60" />
		{m.mod_no_files()}
	</div>
{:else}
	<div class="bg-base-200 rounded-box h-full overflow-y-auto p-3 pr-1">
		<ul class="menu menu-xs w-full p-0 font-mono">
			{#each tree as node}
				{@render renderNode(node)}
			{/each}
		</ul>
	</div>
{/if}
