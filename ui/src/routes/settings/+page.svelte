<script lang="ts">
	import { onMount } from "svelte";
	import {
		getAutostartStatus,
		enableAutostart,
		disableAutostart,
		type AutostartStatus,
	} from "$lib/api";

	let status = $state<AutostartStatus | null>(null);
	let loading = $state(false);
	let error = $state("");
	let message = $state("");

	async function refresh() {
		try {
			status = await getAutostartStatus();
			error = "";
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	async function toggle() {
		if (!status) return;
		loading = true;
		message = "";
		error = "";
		try {
			if (status.installed) {
				message = await disableAutostart();
			} else {
				message = await enableAutostart();
			}
			await refresh();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	onMount(refresh);
</script>

<div class="page">
	<div class="panel">
		<header class="panel-header">
			<a href="/" class="back" title="Back">
				<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
					<path d="M10 3L5 8l5 5" />
				</svg>
			</a>
			<h1>Settings</h1>
		</header>

		{#if error}
			<div class="error">{error}</div>
		{/if}

		{#if message}
			<div class="message">{message}</div>
		{/if}

		<section class="section">
			<h2>Autostart</h2>
			<p class="desc">Start services automatically when you log in.</p>

			<div class="card">
				<div class="card-row">
					<div class="card-info">
						<span class="card-label">Boot agent</span>
						{#if status}
							<span class="card-status" class:on={status.installed && status.active} class:warn={status.installed && !status.active}>
								{#if status.installed && status.active}
									enabled
								{:else if status.installed}
									installed (inactive)
								{:else}
									disabled
								{/if}
							</span>
						{:else}
							<span class="card-status">loading...</span>
						{/if}
					</div>
					<button
						class="toggle-btn"
						class:on={status?.installed}
						onclick={toggle}
						disabled={loading || !status}
					>
						{#if loading}
							...
						{:else if status?.installed}
							Disable
						{:else}
							Enable
						{/if}
					</button>
				</div>

				{#if status?.agent_path}
					<div class="card-detail">
						<span class="detail-label">agent</span>
						<span class="detail-value">{status.agent_path}</span>
					</div>
				{/if}
			</div>

			{#if status}
				<div class="projects-section">
					<h3>Projects with autostart</h3>
					{#if status.projects.length > 0}
						<ul class="project-list">
							{#each status.projects as name}
								<li>{name}</li>
							{/each}
						</ul>
					{:else}
						<p class="empty-hint">
							No projects have <code>autostart = true</code>
						</p>
					{/if}
					<p class="config-hint">
						Configure in <code>~/.config/kagaya/projects.toml</code>:
					</p>
					<pre class="config-example">[myapp]
dir = "~/dev/myapp"
autostart = true</pre>
				</div>
			{/if}
		</section>
	</div>
</div>

<style>
	.page {
		--base: clamp(14px, 0.4rem + 1.1vw, 32px);
		font-size: var(--base);
		height: 100vh;
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: 1.5em 1em;
		overflow-y: auto;
	}

	.panel {
		width: 100%;
		max-width: 42em;
		display: flex;
		flex-direction: column;
	}

	.panel-header {
		display: flex;
		align-items: center;
		gap: 0.6em;
		padding: 0 0.6em 1.2em;
	}

	.back {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 2em;
		height: 2em;
		color: #555;
		text-decoration: none;
		border-radius: 0.4em;
		transition: all 0.12s;
	}

	.back:hover {
		color: #ccc;
		background: #252540;
	}

	.back svg {
		width: 1.3em;
		height: 1.3em;
	}

	h1 {
		font-size: 1.3em;
		font-weight: 700;
		color: #ccc;
		margin: 0;
	}

	.error {
		background: #2a1010;
		border: 1px solid #442222;
		border-radius: 0.5em;
		padding: 0.6em 1em;
		color: #cc6666;
		margin: 0 0.6em 1em;
		font-size: 0.9em;
	}

	.message {
		background: #102a10;
		border: 1px solid #224422;
		border-radius: 0.5em;
		padding: 0.6em 1em;
		color: #66cc66;
		margin: 0 0.6em 1em;
		font-size: 0.9em;
	}

	.section {
		padding: 0 0.6em;
	}

	h2 {
		font-size: 1.1em;
		font-weight: 600;
		color: #aaa;
		margin: 0 0 0.3em;
	}

	h3 {
		font-size: 0.9em;
		font-weight: 600;
		color: #888;
		margin: 1.2em 0 0.5em;
	}

	.desc {
		color: #555;
		font-size: 0.85em;
		margin: 0 0 1em;
	}

	.card {
		background: #14142a;
		border-radius: 0.5em;
		padding: 1em;
	}

	.card-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1em;
	}

	.card-info {
		display: flex;
		flex-direction: column;
		gap: 0.3em;
	}

	.card-label {
		font-weight: 600;
		color: #ccc;
		font-size: 0.95em;
	}

	.card-status {
		font-size: 0.8em;
		color: #555;
	}

	.card-status.on {
		color: #55cc55;
	}

	.card-status.warn {
		color: #ccaa44;
	}

	.card-detail {
		margin-top: 0.8em;
		padding-top: 0.8em;
		border-top: 1px solid #1e1e32;
		display: flex;
		gap: 0.6em;
		align-items: baseline;
	}

	.detail-label {
		font-size: 0.75em;
		color: #555;
		flex-shrink: 0;
	}

	.detail-value {
		font-size: 0.75em;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		color: #666;
		word-break: break-all;
	}

	.toggle-btn {
		border: 1px solid #333;
		background: #1a1a2e;
		color: #aaa;
		cursor: pointer;
		padding: 0.5em 1.2em;
		border-radius: 0.4em;
		font-size: 0.85em;
		font-weight: 500;
		transition: all 0.15s;
		flex-shrink: 0;
	}

	.toggle-btn:hover {
		background: #252540;
		color: #eee;
		border-color: #555;
	}

	.toggle-btn.on {
		border-color: #442222;
		color: #cc8888;
	}

	.toggle-btn.on:hover {
		background: #2a1010;
		border-color: #663333;
		color: #dd6666;
	}

	.toggle-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.projects-section {
		margin-top: 0.5em;
	}

	.project-list {
		list-style: none;
		padding: 0;
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 0.3em;
	}

	.project-list li {
		background: #14142a;
		padding: 0.5em 0.8em;
		border-radius: 0.4em;
		color: #aaa;
		font-size: 0.9em;
		font-weight: 500;
	}

	.empty-hint {
		color: #555;
		font-size: 0.85em;
		margin: 0;
	}

	.config-hint {
		color: #444;
		font-size: 0.8em;
		margin: 1em 0 0.4em;
	}

	.config-example {
		background: #14142a;
		padding: 0.6em 0.8em;
		border-radius: 0.4em;
		color: #888;
		font-size: 0.8em;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		margin: 0;
		line-height: 1.5;
		overflow-x: auto;
	}

	code {
		background: #1a1a2e;
		padding: 0.1em 0.4em;
		border-radius: 0.2em;
		font-size: 0.9em;
	}
</style>
