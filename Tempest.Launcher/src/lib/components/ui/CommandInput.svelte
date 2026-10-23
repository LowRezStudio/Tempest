<script lang="ts">
	import { CornerDownLeft } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { openTextContextMenu } from "$lib/platform/context-menu";
	import type { Child } from "$lib/platform/api";

	interface Props {
		/** Process whose stdin receives the submitted line. */
		child?: Child | null;
		/** Called with the submitted line after it has been written to stdin. */
		onsend?: (line: string) => void;
		placeholder?: string;
		class?: string;
	}

	let {
		child = null,
		onsend,
		placeholder = m.terminal_command_placeholder(),
		class: className = "",
	}: Props = $props();

	let draft = $state("");

	function submit(event: SubmitEvent): void {
		event.preventDefault();
		const target = child;
		const line = draft.trim();
		if (!target || line.length === 0) return;

		draft = "";
		void target.sendLine(line).catch(() => {
			// The process may have exited between typing and submitting.
		});
		onsend?.(line);
	}
</script>

<form class="flex items-center gap-2 {className}" onsubmit={submit}>
	<input
		class="input input-bordered input-sm min-w-0 flex-1 font-mono"
		bind:value={draft}
		{placeholder}
		aria-label={placeholder}
		disabled={!child}
		autocomplete="off"
		autocapitalize="off"
		spellcheck="false"
		oncontextmenu={openTextContextMenu}
	/>
	<button
		class="btn btn-primary btn-sm"
		type="submit"
		disabled={!child || draft.trim().length === 0}
	>
		<CornerDownLeft size={14} />
		{m.common_send()}
	</button>
</form>
