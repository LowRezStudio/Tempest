<script lang="ts">
	import { Check, Loader2, ShieldAlert } from "@lucide/svelte";
	import { antivirusDialog, closeAntivirusDialog } from "$lib/core/antivirus.svelte";
	import { launchGame } from "$lib/core/launch";
	import { m } from "$lib/paraglide/messages";
	import { invoke, openUrl } from "$lib/platform/api";
	import { addToast } from "$lib/stores/ui.svelte";
	import Modal from "./Modal.svelte";

	// How long the button shows its success state before the game starts.
	const ADDED_PAUSE_MS = 1200;

	let adding = $state(false);
	let added = $state(false);
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
				if (result.unrestored?.length) {
					addToast({
						title: m.antivirus_unrestored(),
						message: result.unrestored.join("\n"),
						tone: "warning",
						duration: 0,
					});
				}
				adding = false;
				added = true;
				await new Promise((resolve) => setTimeout(resolve, ADDED_PAUSE_MS));
				relaunch(false);
				added = false;
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
	dismissible={!adding && !added}
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
				disabled={adding || added}
				onclick={() => relaunch(true)}
			>
				{m.antivirus_launch_without()}
			</button>
		{/if}
		<button
			class="btn {added ? 'btn-success pointer-events-none' : 'btn-primary'}"
			type="button"
			disabled={adding}
			onclick={addExclusions}
		>
			{#if adding}
				<Loader2 size={16} class="animate-spin" />
			{:else if added}
				<Check size={16} />
			{/if}
			{#if added}
				{m.antivirus_added()}
			{:else}
				{instance ? m.antivirus_add_launch() : m.antivirus_add()}
			{/if}
		</button>
	{/snippet}
</Modal>
