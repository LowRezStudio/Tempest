<script lang="ts">
	interface Props {
		displayName: string;
		champion: string;
		status: string;
	}

	let { displayName, champion, status }: Props = $props();
</script>

<div class="flex items-center gap-3 bg-base-200/90 backdrop-blur-xs rounded-lg p-2">
	<img
		src={`/champions/${champion || "Generic"}/icon.webp`}
		alt={champion || "No Champion"}
		class="w-12 h-12 rounded-full ring-2 ring-base-300"
		loading="lazy"
	/>
	<div class="min-w-0">
		<p class="font-semibold truncate">{displayName}</p>
		<p class="text-sm opacity-70">{status}</p>
	</div>
</div>
