<script lang="ts">
	import { ChevronLeft, ChevronRight } from "@lucide/svelte";
	import { nav } from "$lib/studio/nav.svelte";
</script>

<div class="join shrink-0">
	<button
		class="btn join-item btn-ghost btn-xs"
		onclick={() => nav.back()}
		disabled={!nav.canBack}
		title={nav.label(-1) ? `Back to ${nav.label(-1)} (Alt+←)` : "Back (Alt+←)"}
		aria-label="Back"
	>
		<ChevronLeft size={14} />
	</button>
	<button
		class="btn join-item btn-ghost btn-xs"
		onclick={() => nav.forward()}
		disabled={!nav.canForward}
		title={nav.label(1) ? `Forward to ${nav.label(1)} (Alt+→)` : "Forward (Alt+→)"}
		aria-label="Forward"
	>
		<ChevronRight size={14} />
	</button>
</div>
