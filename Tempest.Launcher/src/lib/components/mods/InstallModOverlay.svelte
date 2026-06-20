<script lang="ts">
	import { PackageOpen } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";

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
				<h2 class="card-title justify-center">{m.overlay_install_mod_title()}</h2>
				<p class="text-sm opacity-70">
					{@html m.overlay_drop_here({
						upk: '<span class="font-mono text-accent">.upk</span>',
						pck: '<span class="font-mono text-accent">.pck</span>',
					})}
				</p>
			</div>
		</div>
	</div>
{/if}
