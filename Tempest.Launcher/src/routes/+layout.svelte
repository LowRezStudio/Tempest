<script lang="ts">
	import "$lib/styles/global.css";
	import "@fontsource-variable/montserrat";
	import "@fontsource-variable/ubuntu-sans-mono";

	const { children } = $props();
</script>

{@render children?.()}
