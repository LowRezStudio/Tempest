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
		class="bg-base-100/90 pointer-events-none absolute inset-0 z-40 flex flex-col items-center justify-center backdrop-blur-sm"
		style="animation: fade-in 0.12s ease forwards;"
	>
		<div
			class="card bg-base-200/95 border-accent/50 w-full max-w-sm border-2 border-dashed text-center shadow-2xl"
			style="animation: pop-in 0.15s var(--ease-spring) forwards;"
		>
			<div class="card-body items-center gap-3">
				<div
					class="bg-accent/10 text-accent flex h-16 w-16 items-center justify-center rounded-xl"
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
