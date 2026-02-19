<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import {
		getServiceDetail,
		startService,
		stopService,
		reloadService,
		echoWebSocketUrl,
		type ServiceDetail,
	} from '$lib/api';
	import Terminal from '$lib/components/Terminal.svelte';
	import StatusIcon from '$lib/components/StatusIcon.svelte';

	let detail = $state<ServiceDetail | null>(null);
	let error = $state('');
	let loading = $state(false);
	let wsUrl = $state('');
	let innerWidth = $state(800);

	let name = $derived(page.params.name!);
	let scale = $derived(Math.max(0.85, Math.min(1.6, innerWidth / 800)));

	async function refresh() {
		try {
			detail = await getServiceDetail(name);
			if (detail.running) {
				wsUrl = echoWebSocketUrl(name);
			} else {
				wsUrl = '';
			}
			error = '';
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	async function handleAction(action: 'start' | 'stop' | 'reload') {
		loading = true;
		try {
			if (action === 'start') await startService(name);
			else if (action === 'stop') await stopService(name);
			else await reloadService(name);
			setTimeout(refresh, 300);
		} catch (e) {
			console.error(e);
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		refresh();
		const timer = setInterval(refresh, 10000);
		return () => clearInterval(timer);
	});

	function handleKeydown(e: KeyboardEvent) {
		if (e.metaKey || e.ctrlKey || e.altKey) return;

		switch (e.key) {
			case 'Escape':
				e.preventDefault();
				goto('/');
				break;
			case 's':
				if (detail && !detail.running && !loading) {
					e.preventDefault();
					handleAction('start');
				}
				break;
			case 'x':
				if (detail?.running && !loading) {
					e.preventDefault();
					handleAction('stop');
				}
				break;
			case 'r':
				if (detail?.running && !loading) {
					e.preventDefault();
					handleAction('reload');
				}
				break;
		}
	}
</script>

<svelte:window bind:innerWidth onkeydown={handleKeydown} />

<div
	class="page"
	style:--scale={scale}
	style:--icon-size="{Math.round(28 * scale)}px"
>
	<header>
		<a href="/" class="back" title="Back to services">
			<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M10 3L5 8l5 5" /></svg>
		</a>
		<StatusIcon status={detail?.running ? "running" : "stopped"} size="calc(16px * var(--scale, 1))" />
		<h1>{name}</h1>
		{#if detail}
			<span class="actions">
				{#if detail.running}
					<button class="icon stop" onclick={() => handleAction('stop')} disabled={loading} title="Stop">
						<svg viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1.5" /></svg>
					</button>
					<button class="icon reload" onclick={() => handleAction('reload')} disabled={loading} title="Reload">
						<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M2.5 8a5.5 5.5 0 0 1 9.9-3.2M13.5 8a5.5 5.5 0 0 1-9.9 3.2" /><polyline points="12 2 13 5 10 5.5" /><polyline points="4 14 3 11 6 10.5" /></svg>
					</button>
				{:else}
					<button class="icon start" onclick={() => handleAction('start')} disabled={loading} title="Start">
						<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z" /></svg>
					</button>
				{/if}
			</span>
		{/if}
	</header>

	{#if error}
		<div class="error">{error}</div>
	{/if}

	{#if detail}
		{#if detail.running && wsUrl}
			<div class="terminal-fill">
				<Terminal {wsUrl} />
			</div>
		{:else if !detail.running}
			<div class="stopped-state">
				<StatusIcon status="stopped" size="calc(40px * var(--scale, 1))" />
				<span class="stopped-label">stopped</span>
				<button class="start-btn" onclick={() => handleAction('start')} disabled={loading}>
					<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z" /></svg>
					Start
				</button>
			</div>
		{/if}
	{/if}
</div>

<style>
	.page {
		padding: calc(12px * var(--scale, 1)) calc(28px * var(--scale, 1));
		max-width: 1400px;
		margin: 0 auto;
		height: 100vh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	header {
		display: flex;
		align-items: center;
		gap: calc(14px * var(--scale, 1));
		padding-bottom: calc(12px * var(--scale, 1));
		border-bottom: 1px solid #222238;
		flex-shrink: 0;
	}

	.back {
		color: #555;
		text-decoration: none;
		display: flex;
		align-items: center;
		padding: 0;
		transition: color 0.15s;
	}

	.back svg {
		width: var(--icon-size, 24px);
		height: var(--icon-size, 24px);
	}

	.back:hover {
		color: #ccc;
	}

	h1 {
		font-size: calc(1.35rem * var(--scale, 1));
		font-weight: 600;
		color: #e0e0e0;
		margin: 0;
	}

	.actions {
		display: flex;
		gap: calc(14px * var(--scale, 1));
		margin-left: auto;
	}

	.icon {
		width: var(--icon-size, 24px);
		height: var(--icon-size, 24px);
		border: none;
		background: none;
		color: #555;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		transition: color 0.15s;
	}

	.icon svg {
		width: 100%;
		height: 100%;
	}

	.icon:hover { color: #ccc; }
	.icon.start:hover { color: #55cc55; }
	.icon.stop:hover { color: #dd6666; }
	.icon.reload:hover { color: #7777cc; }
	.icon:disabled { opacity: 0.25; cursor: not-allowed; }

	.error {
		background: #2a1010;
		border: 1px solid #552222;
		border-radius: 6px;
		padding: 12px 16px;
		margin-top: calc(12px * var(--scale, 1));
		color: #cc6666;
		font-size: 0.9rem;
		flex-shrink: 0;
	}

	.terminal-fill {
		flex: 1;
		min-height: 0;
		margin-top: calc(12px * var(--scale, 1));
	}

	.stopped-state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: calc(16px * var(--scale, 1));
	}

	.stopped-label {
		font-size: calc(1.2rem * var(--scale, 1));
		color: #555;
		text-transform: uppercase;
		letter-spacing: 0.1em;
	}

	.start-btn {
		display: flex;
		align-items: center;
		gap: calc(8px * var(--scale, 1));
		padding: calc(12px * var(--scale, 1)) calc(24px * var(--scale, 1));
		background: none;
		border: 1px solid #333;
		border-radius: 6px;
		color: #888;
		font-size: calc(1rem * var(--scale, 1));
		cursor: pointer;
		transition: color 0.15s, border-color 0.15s;
	}

	.start-btn svg {
		width: calc(20px * var(--scale, 1));
		height: calc(20px * var(--scale, 1));
	}

	.start-btn:hover {
		color: #55cc55;
		border-color: #55cc55;
	}

	.start-btn:disabled {
		opacity: 0.25;
		cursor: not-allowed;
	}
</style>
