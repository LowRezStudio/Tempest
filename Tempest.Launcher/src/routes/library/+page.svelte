<script>
	import { addInstance, instances } from "$lib/state/instances.svelte";
	import { Gamepad2 } from "@lucide/svelte";
</script>
<div class="p-3">
	<div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] w-full gap-3 mr-auto scroll-smooth overflow-y-auto">
		{#each instances.current as instance}
			<a
				class="flex items-center justify-center p-3 gap-3 rounded-xl bg-[#16161a] hover:bg-[#1c1c21]"
				href={`/library/${instance.id}`}
			>
				<div class="grid place-items-center">
					<div class="grid place-items-center bg-slate-300 rounded-xl size-12">
						<img class="size-10" src="/img/crystal.png" alt="logo" />
					</div>
				</div>
				<div class="flex flex-col justify-between flex-auto">
					<p class="font-bold">{instance.label}</p>
					<div class="flex items-center gap-1.5 font-[550] text-[0.85rem] text-[#96a2b0]">
						<Gamepad2 size="20" />
						<p class="">{instance.version ?? "Unknown"}</p>
					</div>
				</div>
			</a>
		{/each}
	</div>
</div>
