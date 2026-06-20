<script lang="ts">
	import { PackageOpen } from "@lucide/svelte";

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();
</script>

{#if visible}
	<div
		class="absolute inset-0 z-40 flex flex-col items-center justify-center bg-base-100/90 backdrop-blur-sm pointer-events-none"
		style="animation: fade-in 0.12s ease forwards;"
	>
		<div
			class="card bg-base-200/95 shadow-2xl max-w-sm w-full text-center border-2 border-dashed border-accent/50"
			style="animation: pop-in 0.15s var(--ease-spring) forwards;"
		>
			<div class="card-body items-center gap-3">
				<div
					class="w-16 h-16 rounded-xl bg-accent/10 flex items-center justify-center text-accent"
				>
					<PackageOpen size={32} />
				</div>
				<h2 class="card-title justify-center">Install Mod</h2>
				<p class="text-sm opacity-70">
					Drop the mod file (<span class="font-mono text-accent">.upk</span>,
					<span class="font-mono text-accent">.pck</span>) here to install onto this
					instance.
				</p>
			</div>
		</div>
	</div>
{/if}
