<script lang="ts">
	import { AlertTriangle, Replace } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";

	interface Props {
		open: boolean;
		modName: string;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let { open = $bindable(false), modName, onconfirm, oncancel }: Props = $props();

	function handleConfirm() {
		onconfirm();
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal bind:open title="Mod Conflict" class="max-w-md">
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<AlertTriangle size={24} />
			</div>
			<div>
				<h4 class="font-bold text-base">Replace existing mod?</h4>
				<p class="text-sm opacity-70 mt-1">
					A mod named <span class="font-mono font-bold text-accent">{modName}</span> is already
					installed on this instance.
				</p>
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
			Replace Mod
		</button>
	{/snippet}
</Modal>
