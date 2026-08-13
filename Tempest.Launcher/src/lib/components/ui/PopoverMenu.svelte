<script lang="ts">
	import { Popover } from "bits-ui";
	import type { Snippet } from "svelte";

	interface Props {
		trigger: Snippet;
		children: Snippet;
		align?: "start" | "end" | "center";
		class?: string;
	}

	let { trigger, children, align = "end", class: className = "" }: Props = $props();
</script>

<Popover.Root>
	<Popover.Trigger class="list-none" onclick={(e) => e.stopPropagation()}>
		{@render trigger()}
	</Popover.Trigger>
	<Popover.Portal>
		<Popover.Content
			class="bg-base-300 rounded-box border-base-content/10 popover-animate z-50 min-w-[180px] border p-1 shadow-xl {className}"
			{align}
			sideOffset={4}
		>
			<ul role="menu" class="menu w-52">
				{@render children()}
			</ul>
		</Popover.Content>
	</Popover.Portal>
</Popover.Root>
