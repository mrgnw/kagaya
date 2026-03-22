<script lang="ts">
	let {
		status,
		size = "1em",
		loading = false,
	}: {
		status: string;
		size?: string;
		loading?: boolean;
	} = $props();

	let resolved = $derived.by(() => {
		if (loading) return { glyph: "●", color: "#555", cls: "loading" };
		if (status === "on") return { glyph: "●", color: "#44bb44", cls: "on" };
		if (status === "degraded") return { glyph: "▲", color: "#ccaa44", cls: "degraded" };
		if (status === "err") return { glyph: "✖", color: "#cc4444", cls: "err" };
		if (status === "off") return { glyph: "◻", color: "#555", cls: "off" };
		if (status.startsWith("running")) return { glyph: "●", color: "#44bb44", cls: "running" };
		if (status === "stopped") return { glyph: "◻", color: "#555", cls: "stopped" };
		if (status.startsWith("crashed")) return { glyph: "⚠", color: "#ccaa44", cls: "crashed" };
		if (status.startsWith("failed")) return { glyph: "✖", color: "#cc4444", cls: "failed" };
		return { glyph: "●", color: "#555", cls: "unknown" };
	});
</script>

<span
	class="status-icon {resolved.cls}"
	style:color={resolved.color}
	style:font-size={size}
	title={status}
>{resolved.glyph}</span>

<style>
	.status-icon {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		line-height: 1;
		flex-shrink: 0;
		width: 1.2em;
		text-align: center;
	}
	.loading {
		animation: pulse 0.8s ease-in-out infinite;
	}
	@keyframes pulse {
		0%, 100% { opacity: 1; }
		50% { opacity: 0.3; }
	}
</style>
