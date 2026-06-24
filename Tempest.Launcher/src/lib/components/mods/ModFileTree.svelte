<script lang="ts">
	import { File, Folder } from "@lucide/svelte";

	interface Props {
		files: string[];
		basePath?: string;
	}

	let { files, basePath }: Props = $props();

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
</script>

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

<div class="overflow-y-auto pr-1 bg-base-200 rounded-box p-3 h-full">
	<ul class="menu menu-xs p-0 font-mono w-full">
		{#each tree as node}
			{@render renderNode(node)}
		{/each}
	</ul>
</div>
