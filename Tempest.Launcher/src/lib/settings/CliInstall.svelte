<script lang="ts">
	import { Terminal } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke, platform } from "$lib/platform/api";
	import { addToast } from "$lib/stores/ui.svelte";
	import type { CliInstallResult } from "$ipc/contract";

	let status = $state<CliInstallResult | null>(null);
	let installing = $state(false);

	const isMac = platform() === "macos";

	refreshStatus();

	async function refreshStatus() {
		try {
			status = await invoke("cli:status");
		} catch {
			status = null;
		}
	}

	async function install() {
		installing = true;
		try {
			status = await invoke("cli:install");
			if (status.installed) {
				addToast({ message: m.settings_cli_installed_toast(), tone: "success" });
			} else if (status.error === "cancelled") {
				// user dismissed the admin prompt; nothing to show
			} else if (status.error) {
				addToast({ message: status.error, tone: "error" });
			} else {
				addToast({ message: m.settings_cli_not_bundled_toast(), tone: "warning" });
			}
		} catch (error) {
			addToast({
				message: error instanceof Error ? error.message : String(error),
				tone: "error",
			});
		} finally {
			installing = false;
		}
	}
</script>

<div class="flex flex-col gap-4">
	<h2 class="text-xl font-semibold">
		<Terminal size={18} class="mr-1 inline align-[-2px]" />
		{m.settings_cli_title()}
	</h2>
	<p class="text-sm">
		{m.settings_cli_description()}
	</p>

	{#if status?.installed && status.path}
		<div class="bg-base-200 rounded-box flex items-center gap-2 px-3 py-2">
			<span class="badge badge-success badge-sm">{m.settings_cli_ready()}</span>
			<code class="font-mono text-xs">{status.path}</code>
			{#if !status.onPath}
				<span class="text-base-content/60 text-xs">{m.settings_cli_not_on_path()}</span>
			{/if}
		</div>
	{:else}
		<div>
			<button
				type="button"
				class="btn btn-primary btn-sm gap-2"
				onclick={install}
				disabled={installing}
			>
				{#if installing}
					<span class="loading loading-spinner loading-xs"></span>
				{/if}
				{isMac ? m.settings_cli_install_mac() : m.settings_cli_install()}
			</button>
		</div>
	{/if}
</div>
