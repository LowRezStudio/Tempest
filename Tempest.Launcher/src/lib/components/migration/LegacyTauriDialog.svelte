<script lang="ts">
	import { Loader2, PackageX } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { persistedJSON } from "$lib/stores/persisted.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import type { LegacyInstall } from "$ipc/contract";

	const prompted = persistedJSON<boolean>("legacyTauriPrompted", false);

	let open = $state(false);
	let installs = $state<LegacyInstall[]>([]);
	let uninstalling = $state(false);

	// Ask once: look for a leftover Tauri MSI install on first launch. The main
	// process does the platform check itself (returns [] elsewhere), so there is
	// no racy platform() read here.
	$effect(() => {
		if (prompted.value) return;
		void invoke("legacy:find").then((found) => {
			if (found.length > 0) {
				installs = found;
				open = true;
			}
		});
	});

	function keep() {
		prompted.value = true;
		open = false;
	}

	async function handleUninstall() {
		uninstalling = true;
		try {
			const results = await invoke("legacy:uninstall");
			// 0/3010 = success (3010 = reboot required), 1602 = cancelled.
			const failed = results.find((r) => r.code !== 0 && r.code !== 3010 && r.code !== 1602);
			if (failed) {
				addToast({
					message: m.legacy_remove_failed({ code: failed.code }),
					tone: "error",
				});
				prompted.value = true;
				open = false;
			} else if (results.some((r) => r.code === 1602)) {
				addToast({ message: m.legacy_cancelled(), tone: "warning" });
				// Keep the dialog open so the user can retry.
			} else {
				addToast({ message: m.legacy_removed(), tone: "success" });
				keep();
			}
		} catch (error) {
			console.error("Legacy uninstall failed:", error);
			addToast({ message: m.legacy_remove_failed({ code: 1603 }), tone: "error" });
			keep();
		} finally {
			uninstalling = false;
		}
	}
</script>

<Modal bind:open title={m.legacy_found_title()} onclose={keep}>
	<div class="flex flex-col gap-4 py-2">
		<div class="flex items-start gap-3">
			<PackageX size={28} class="text-primary mt-0.5 shrink-0" />
			<div class="space-y-2">
				<p class="text-sm">{m.legacy_found_desc()}</p>
				<p class="text-base-content/70 text-sm">
					{m.legacy_found_body()}
					{#each installs as install, i (install.id)}
						{#if i > 0}
							{", "}
						{/if}
						<span class="text-base-content font-medium"
							>{install.name || m.legacy_name_fallback()}</span
						>
					{/each}
				</p>
			</div>
		</div>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" onclick={keep} disabled={uninstalling}>
			{m.legacy_keep()}
		</button>
		<button class="btn btn-primary" onclick={handleUninstall} disabled={uninstalling}>
			{#if uninstalling}
				<Loader2 size={16} class="animate-spin" />
			{/if}
			{uninstalling ? m.legacy_uninstalling() : m.legacy_uninstall()}
		</button>
	{/snippet}
</Modal>
