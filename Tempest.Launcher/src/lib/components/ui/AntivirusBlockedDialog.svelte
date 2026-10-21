<script lang="ts">
	import { Loader2, ShieldAlert } from "@lucide/svelte";
	import { antivirusDialog, closeAntivirusDialog } from "$lib/core/antivirus.svelte";
	import { launchGame } from "$lib/core/launch";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openUrl } from "$lib/platform/api";
	import { addToast } from "$lib/stores/ui.svelte";
	import Modal from "./Modal.svelte";

	let adding = $state(false);
	let instance = $derived(antivirusDialog.value.instance);

	function relaunch(noDlls: boolean) {
		const target = antivirusDialog.value.instance;
		closeAntivirusDialog();
		if (!target) return;
		launchGame(target, { noDlls }).catch((error: unknown) => {
			addToast({ title: m.antivirus_launch_failed(), message: String(error), tone: "error" });
		});
	}

	function showFailure(error: string | undefined) {
		addToast({
			title: m.antivirus_failed(),
			message: error || m.antivirus_failed_hint(),
			tone: "error",
			duration: 0,
		});
	}

	async function addExclusions() {
		adding = true;
		try {
			// A $state proxy can't cross IPC (structured clone rejects it).
			const result = await invoke("defender:add-exclusions", {
				paths: $state.snapshot(antivirusDialog.value.folders),
			});
			if (result.status === "added") {
				if (instance) {
					addToast({ message: m.antivirus_added_launching(), tone: "success" });
					relaunch(false);
				} else {
					addToast({ message: m.antivirus_added(), tone: "success", duration: 8000 });
					closeAntivirusDialog();
				}
			} else if (result.status === "cancelled") {
				addToast({ message: m.antivirus_cancelled(), tone: "warning" });
			} else {
				showFailure(result.error);
			}
		} catch (error) {
			console.error("Adding Defender exclusions failed:", error);
			showFailure(String(error));
		} finally {
			adding = false;
		}
	}
</script>

<Modal
	bind:open={antivirusDialog.value.open}
	title={m.antivirus_title()}
	class="max-w-xl"
	dismissible={!adding}
	closeOnOutsideClick={false}
	onclose={closeAntivirusDialog}
>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<ShieldAlert size={24} class="text-warning mt-0.5 shrink-0" />
			<div class="space-y-2 text-sm">
				<p>{m.antivirus_desc()}</p>
				{#if instance}
					<p>{m.antivirus_stopped()}</p>
				{/if}
				<p class="opacity-70">{m.antivirus_body()}</p>
			</div>
		</div>
		<ul class="bg-base-200 rounded-box space-y-1 p-3 font-mono text-xs break-all">
			{#each antivirusDialog.value.folders as folder (folder)}
				<li>{folder}</li>
			{/each}
		</ul>
		<p class="text-sm opacity-60">
			{m.antivirus_note()}
			<button
				class="link"
				type="button"
				onclick={() => void openUrl("windowsdefender://threatsettings")}
			>
				{m.antivirus_open_settings()}
			</button>
		</p>
	</div>

	{#snippet actions()}
		{#if instance}
			<button
				class="btn btn-ghost"
				type="button"
				disabled={adding}
				onclick={() => relaunch(true)}
			>
				{m.antivirus_launch_without()}
			</button>
		{/if}
		<button class="btn btn-primary" type="button" disabled={adding} onclick={addExclusions}>
			{#if adding}
				<Loader2 size={16} class="animate-spin" />
			{/if}
			{instance ? m.antivirus_add_launch() : m.antivirus_add()}
		</button>
	{/snippet}
</Modal>
