<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { invoke } from "$lib/platform/api";
	import type { McpInfo } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let info = $state<McpInfo | null>(null);
	let copied = $state<string | null>(null);

	$effect(() => {
		if (open) void invoke("mcp:info").then((i) => (info = i));
	});

	const claudeCommand = $derived(
		info
			? `claude mcp add --transport http tempest-studio ${info.url} --header "Authorization: Bearer ${info.token}"`
			: "",
	);
	const json = $derived(
		info
			? JSON.stringify(
					{
						mcpServers: {
							"tempest-studio": {
								type: "http",
								url: info.url,
								headers: { Authorization: `Bearer ${info.token}` },
							},
						},
					},
					null,
					2,
				)
			: "",
	);

	async function copy(label: string, text: string) {
		await navigator.clipboard.writeText(text);
		copied = label;
		setTimeout(() => (copied = null), 1500);
	}
</script>

<Modal bind:open title="MCP server" width="max-w-2xl">
	{#if info?.running}
		<p class="text-sm opacity-80">
			Agents can drive this Studio over the Model Context Protocol: list windows, open
			workspaces and packages, call every backend method, read and edit the assembly, run code
			against the stores, take screenshots. Listening on localhost only; the token is kept in
			the app's data folder (delete
			<code class="text-xs">mcp-token</code> there to rotate it) and written with the URL to
			<code class="text-xs">{info.configPath}</code>.
		</p>
		<div class="mt-3 space-y-3 text-xs">
			<div>
				<div class="mb-1 flex items-center gap-2 font-semibold">
					Claude Code
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => copy("claude", claudeCommand)}
						>{copied === "claude" ? "Copied" : "Copy"}</button
					>
				</div>
				<pre
					class="bg-base-300 selectable overflow-x-auto rounded p-2 font-mono">{claudeCommand}</pre>
			</div>
			<div>
				<div class="mb-1 flex items-center gap-2 font-semibold">
					.mcp.json / Cursor / Windsurf
					<button class="btn btn-ghost btn-xs" onclick={() => copy("json", json)}
						>{copied === "json" ? "Copied" : "Copy"}</button
					>
				</div>
				<pre
					class="bg-base-300 selectable overflow-x-auto rounded p-2 font-mono">{json}</pre>
			</div>
			<p class="opacity-60">
				Tools: studio_windows, studio_state, upk_call, studio_open_workspace,
				studio_open_package, studio_select, studio_set_mode, studio_open_table, studio_eval,
				studio_screenshot, studio_new_window.
			</p>
		</div>
	{:else if info}
		<div class="alert alert-warning text-sm">
			The MCP server is not running (the port may be taken; set TEMPEST_STUDIO_MCP_PORT and
			restart).
		</div>
	{:else}
		<span class="loading loading-sm loading-spinner"></span>
	{/if}
</Modal>
