<script lang="ts" module>
	// Whether the last interaction was a key press, to tell keyboard-opened dialogs apart.
	let lastInputWasKey = false;
	if (typeof window !== "undefined") {
		window.addEventListener("keydown", () => (lastInputWasKey = true), true);
		window.addEventListener("pointerdown", () => (lastInputWasKey = false), true);
	}
</script>

<script lang="ts">
	import { X } from "@lucide/svelte";
	import { Dialog } from "bits-ui";
	import type { Snippet } from "svelte";

	interface Props {
		open?: boolean;
		title?: string;
		/** With a title, gives the dialog the mod website's header: the icon in a tinted circle */
		icon?: typeof X;
		/** A line under the title */
		description?: string;
		children: Snippet;
		actions?: Snippet;
		class?: string;
		dismissible?: boolean;
		/** Whether a click outside closes a dismissible dialog; Esc and the close button still do. */
		closeOnOutsideClick?: boolean;
		onsubmit?: (e: SubmitEvent) => void;
		onclose?: () => void;
	}

	let {
		open = $bindable(false),
		title,
		icon: Icon,
		description,
		children,
		actions,
		class: className = "",
		dismissible = true,
		closeOnOutsideClick = true,
		onsubmit,
		onclose,
	}: Props = $props();

	let openedByKey = false;
	$effect.pre(() => {
		if (open) openedByKey = lastInputWasKey;
	});

	function handleOpenChange(isOpen: boolean) {
		if (!isOpen && onclose) {
			onclose();
		}
		open = isOpen;
	}
</script>

<Dialog.Root bind:open onOpenChange={handleOpenChange}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
		<Dialog.Content
			class="bg-base-100 rounded-box fixed top-1/2 left-1/2 z-50 max-h-[85vh] w-full max-w-2xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto p-6 shadow-xl {className}"
			style="animation: pop-in 0.15s ease forwards;"
			onEscapeKeydown={(event) => {
				if (!dismissible) event.preventDefault();
			}}
			onInteractOutside={(event) => {
				if (!dismissible || !closeOnOutsideClick) event.preventDefault();
			}}
			onCloseAutoFocus={(event) => {
				// Handing focus back after Esc would show a focus ring on whatever was clicked to
				// open the dialog; only keyboard users need their place back.
				if (!openedByKey) event.preventDefault();
			}}
		>
			<form
				onsubmit={(e) => {
					e.preventDefault();
					if (onsubmit) {
						onsubmit(e);
					}
				}}
				class="contents"
			>
				{#if title && Icon}
					<header class="flex items-start gap-3 pr-10 pb-5">
						<span
							class="bg-primary/12 text-primary flex size-10 shrink-0 items-center justify-center rounded-full"
						>
							<Icon size={18} />
						</span>
						<div class="min-w-0">
							<Dialog.Title class="text-lg leading-tight font-semibold">
								{title}
							</Dialog.Title>
							{#if description}
								<Dialog.Description class="text-base-content/60 mt-0.5 text-sm">
									{description}
								</Dialog.Description>
							{/if}
						</div>
					</header>
				{:else if title}
					<Dialog.Title class="pb-4 text-lg font-bold">
						{title}
					</Dialog.Title>
				{/if}

				{#if dismissible}
					<Dialog.Close
						class="btn btn-circle btn-ghost absolute top-4 right-4"
						type="button"
					>
						<X size={16} />
					</Dialog.Close>
				{/if}

				{@render children()}

				{#if actions}
					<div class="flex justify-end gap-2 pt-4">
						{@render actions()}
					</div>
				{/if}
			</form>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<style>
	@keyframes pop-in {
		from {
			opacity: 0;
			transform: translate(-50%, -50%) scale(0.95);
		}
		to {
			opacity: 1;
			transform: translate(-50%, -50%) scale(1);
		}
	}
</style>
