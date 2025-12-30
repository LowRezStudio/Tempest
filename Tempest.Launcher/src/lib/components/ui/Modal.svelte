<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		open?: boolean;
		title?: string;
		children: Snippet;
		actions?: Snippet;
		class?: string;
	}

	let {
		open = $bindable(false),
		title,
		children,
		actions,
		class: className = "",
	}: Props = $props();

	let dialog: HTMLDialogElement;

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) {
			dialog.showModal();
		} else if (!open && dialog.open) {
			dialog.close();
		}
	});

	function onclose() {
		open = false;
	}

	export function show() {
		open = true;
	}

	export function close() {
		open = false;
	}
</script>

<dialog bind:this={dialog} class="modal" {onclose}>
	<div class="modal-box {className}">
		{#if title}
			<h3 class="font-bold text-lg pb-4">{title}</h3>
		{/if}

		<form method="dialog">
			<button class="btn btn-sm btn-circle btn-ghost absolute right-2 top-2">✕</button>
		</form>

		{@render children()}

		{#if actions}
			<div class="modal-action">
				{@render actions()}
			</div>
		{/if}
	</div>
	<form method="dialog" class="modal-backdrop">
		<button>close</button>
	</form>
</dialog>

<style>
	.modal[open] .modal-box {
		animation: pop-in 0.15s var(--ease-snappy) forwards;
	}

	.modal::backdrop {
		animation: fade-in 0.15s var(--ease-smooth) forwards;
	}
</style>
