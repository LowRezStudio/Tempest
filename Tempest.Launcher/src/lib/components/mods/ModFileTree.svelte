<script lang="ts">
	import { ChevronDown, ChevronRight, File, FileText, Folder } from "@lucide/svelte";
	import { readFile } from "@tauri-apps/plugin-fs";

	interface Props {
		files: string[];
		basePath?: string;
		modId?: string;
	}

	let { files, basePath, modId }: Props = $props();

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
			let target = key;
			const lower = name.toLowerCase();
			const isIni = lower.endsWith(".ini");
			// For .ini, show the mod fragment that is to be merged (stored in .tempest/v2/mods/<modId>/files/...),
			// not the final merged game ini.
			if (isIni && modId && basePath) {
				const sep = basePath.includes("\\") ? "\\" : "/";
				let relative = key;
				const normalizedKey = key.replaceAll("\\", "/");
				const normalizedBase = basePath.replaceAll("\\", "/");
				if (normalizedKey.toLowerCase().startsWith(normalizedBase.toLowerCase() + "/")) {
					relative = key.slice(basePath.length).replace(/^[\\/]+/, "");
				} else {
					relative = key.replace(/^[\\/]+/, "");
				}
				// normalize relative to OS sep
				relative = relative.replaceAll("/", sep).replaceAll("\\", sep);
				target =
					basePath.replace(/[\\/]+$/, "") +
					sep +
					".tempest" +
					sep +
					"v2" +
					sep +
					"mods" +
					sep +
					modId +
					sep +
					"files" +
					sep +
					relative;
			} else {
				// Resolve relative paths against basePath (InstalledFiles can be relative)
				const isAbsolute =
					/^[a-zA-Z]:[\\/]/.test(target) ||
					target.startsWith("/") ||
					target.startsWith("\\");
				if (!isAbsolute && basePath) {
					const sep = basePath.includes("\\") ? "\\" : "/";
					target = basePath.replace(/[\\/]+$/, "") + sep + target.replace(/^[\\/]+/, "");
				}
			}
			const data = await readFile(target);
			const raw = new TextDecoder("utf-8", { fatal: false }).decode(data);
			// Truncate very large files (50k chars) to keep UI responsive
			let text = raw;
			if (text.length > 50_000) {
				text =
					text.slice(0, 50_000) +
					"\n\n… truncated (file too large, showing first 50k chars)";
			}
			fileContents.set(key, { loading: false, text, error: "" });
		} catch (e) {
			fileContents.set(key, {
				loading: false,
				text: "",
				error: e instanceof Error ? e.message : String(e),
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
		<li>
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
								Loading...
							</div>
						{:else if state?.error}
							<div
								class="bg-error/10 text-error border-error/20 rounded border px-3 py-2 font-mono text-xs"
							>
								Failed to read: {state.error}
							</div>
						{:else if state?.text !== undefined}
							{#if state.text.length === 0}
								<div
									class="bg-base-300/30 border-base-300 rounded border px-3 py-2 text-xs italic opacity-50"
								>
									Empty file
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

<div class="bg-base-200 rounded-box h-full overflow-y-auto p-3 pr-1">
	<ul class="menu menu-xs w-full p-0 font-mono">
		{#each tree as node}
			{@render renderNode(node)}
		{/each}
	</ul>
</div>
