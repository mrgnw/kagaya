<script lang="ts">
	let { children } = $props();
</script>

<svelte:head>
	<title>kagaya</title>
</svelte:head>

{@render children()}

<style>
	:global(html, body) {
		margin: 0;
		padding: 0;
		background: #0e0e1a;
		color: #e0e0e0;
		font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', sans-serif;
		font-size: 15px;
		line-height: 1.5;
		-webkit-font-smoothing: antialiased;
		height: 100%;
		overflow: hidden;
	}

	:global(*) {
		box-sizing: border-box;
	}

	:global(a) {
		color: inherit;
	}
</style>
