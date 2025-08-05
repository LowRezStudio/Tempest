<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import { addBuild } from "$lib/state/builds.svelte";
</script>

<h1>okay garmin, video speichern</h1>
<Button onclick={() => addBuild({ label: "test", path: "" })}>
	Test
</Button>
