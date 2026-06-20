<script lang="ts">
	import { AlertTriangle, Replace } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";

	interface Props {
		open: boolean;
		modName: string;
		isModConflict?: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let {
		open = $bindable(false),
		modName,
		isModConflict = true,
		onconfirm,
		oncancel,
	}: Props = $props();

	function handleConfirm() {
		onconfirm();
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal bind:open title={isModConflict ? "Mod Conflict" : "File Conflict"} class="max-w-md">
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<AlertTriangle size={24} />
			</div>
			<div>
				<h4 class="font-bold text-base">
					{isModConflict ? "Replace existing mod?" : "Replace existing file?"}
				</h4>
				{#if isModConflict}
					<p class="text-sm opacity-70 mt-1">
						A mod named <span class="font-mono font-bold text-accent">{modName}</span> is
						already installed on this instance.
					</p>
				{:else}
					<p class="text-sm opacity-70 mt-1">
						A file named <span class="font-mono font-bold text-accent">{modName}</span> is
						already present on this instance.
					</p>
				{/if}
			</div>
		</div>
		<p class="text-sm opacity-60">
			Do you want to overwrite it with the new file? This will replace the files in your game
			directory.
		</p>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" onclick={handleCancel}> Cancel </button>
		<button class="btn btn-error" onclick={handleConfirm}>
			<Replace size={16} />
			{isModConflict ? "Replace Mod" : "Replace File"}
		</button>
	{/snippet}
</Modal>
