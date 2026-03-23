<script lang="ts">
	import { onMount } from "svelte";
	import { SvelteSet } from "svelte/reactivity";
	import {
		getServices,
		getAutostartStatus,
		getVersion,
		startService,
		stopService,
		reloadService,
		type ServiceInfo,
		type AutostartStatus,
	} from "$lib/api";
	import ServiceRow from "$lib/components/ServiceRow.svelte";
	import StatusIcon from "$lib/components/StatusIcon.svelte";
	import logoSvg from "$lib/assets/logo.svg";

	type AggregateState = "on" | "degraded" | "err" | "off";
	type ServiceWithState = ServiceInfo & { state: AggregateState };

	const stateOrder = ["on", "degraded", "err", "off"] as const satisfies AggregateState[];

	let services = $state<ServiceWithState[]>([]);
	let autostartStatus = $state<AutostartStatus | null>(null);
	let error = $state("");
	let refreshTimer: ReturnType<typeof setInterval>;
	let selectedNames = new SvelteSet<string>();
	let bulkLoading = $state(false);
	let version = $state("");
	let headerCheckbox = $state<HTMLInputElement | null>(null);

	let hasSelection = $derived(selectedNames.size > 0);
	let allSelected = $derived(
		services.length > 0 && selectedNames.size === services.length,
	);
	let someSelected = $derived(hasSelection && !allSelected);
	let runningCount = $derived(services.filter((s) => s.running).length);
	let stoppedCount = $derived(services.filter((s) => !s.running).length);
	let stateCounts = $derived.by(() => {
		let counts: Record<AggregateState, number> = {
			on: 0,
			degraded: 0,
			err: 0,
			off: 0,
		};
		for (const service of services) counts[service.state] += 1;
		return counts;
	});

	let selectedServices = $derived(
		services.filter((s) => selectedNames.has(s.name)),
	);
	let selectedRunning = $derived(
		selectedServices.filter((s) => s.running).length,
	);
	let selectedStopped = $derived(
		selectedServices.filter((s) => !s.running).length,
	);

	function syncIndeterminate() {
		if (headerCheckbox) {
			headerCheckbox.indeterminate = someSelected;
		}
	}

	async function refresh() {
		try {
			services = (await getServices()) as ServiceWithState[];
			error = "";
			for (const name of selectedNames) {
				if (!services.some((s) => s.name === name)) selectedNames.delete(name);
			}
			queueMicrotask(syncIndeterminate);
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
		try {
			autostartStatus = await getAutostartStatus();
		} catch {
			autostartStatus = null;
		}
	}

	function toggleSelect(name: string, checked: boolean) {
		if (checked) selectedNames.add(name);
		else selectedNames.delete(name);
		queueMicrotask(syncIndeterminate);
	}

	function headerCheckClicked() {
		if (allSelected || someSelected) {
			selectedNames.clear();
		} else {
			selectedNames.clear();
			for (const service of services) selectedNames.add(service.name);
		}
		queueMicrotask(syncIndeterminate);
	}

	async function bulkAction(action: "start" | "stop" | "reload") {
		bulkLoading = true;
		const targets = [...selectedNames];
		try {
			await Promise.allSettled(
				targets.map((name) => {
					if (action === "start") return startService(name);
					if (action === "stop") return stopService(name);
					return reloadService(name);
				}),
			);
			setTimeout(refresh, 300);
		} catch (e) {
			console.error(e);
		} finally {
			bulkLoading = false;
		}
	}

	async function actionAll(action: "start" | "stop") {
		bulkLoading = true;
		const targets =
			action === "start"
				? services.filter((s) => !s.running)
				: services.filter((s) => s.running);
		try {
			await Promise.allSettled(
				targets.map((s) =>
					action === "start"
						? startService(s.name)
						: stopService(s.name),
				),
			);
			setTimeout(refresh, 300);
		} catch (e) {
			console.error(e);
		} finally {
			bulkLoading = false;
		}
	}

	onMount(() => {
		refresh();
		getVersion().then((v) => (version = v)).catch(() => {});
		refreshTimer = setInterval(refresh, 5000);
		return () => clearInterval(refreshTimer);
	});

	function handleKeydown(e: KeyboardEvent) {
		if (e.metaKey || e.ctrlKey || e.altKey) return;
		const tag = (e.target as HTMLElement)?.tagName;
		if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

		switch (e.key) {
			case "a":
				e.preventDefault();
				headerCheckClicked();
				break;
			case "s":
				e.preventDefault();
				if (hasSelection) {
					if (selectedStopped > 0) bulkAction("start");
				} else if (stoppedCount > 0) {
					actionAll("start");
				}
				break;
			case "x":
				e.preventDefault();
				if (hasSelection) {
					if (selectedRunning > 0) bulkAction("stop");
				} else if (runningCount > 0) {
					actionAll("stop");
				}
				break;
			case "r":
				e.preventDefault();
				if (hasSelection && selectedRunning > 0) bulkAction("reload");
				break;
			case "Escape":
				e.preventDefault();
				selectedNames.clear();
				queueMicrotask(syncIndeterminate);
				break;
			default:
				if (e.key >= "1" && e.key <= "9") {
					const idx = parseInt(e.key) - 1;
					if (idx < services.length) {
						e.preventDefault();
						toggleSelect(
							services[idx].name,
							!selectedNames.has(services[idx].name),
						);
					}
				}
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="page">
	{#if error}
		<div class="error-wrap">
			<div class="error">
				{error}
				<p>Make sure the kagaya server is running on port 13369</p>
			</div>
		</div>
	{/if}

	<div class="panel">
		<header class="panel-header">
			<div class="header-left">
				<img src={logoSvg} alt="" class="logo" />
				<span class="brand-name">kagaya{#if version} <span class="version">{version}</span>{/if}</span>
				<div class="stats" aria-label="Project status">
					{#each stateOrder as state (state)}
						{#if stateCounts[state] > 0}
							<span class="stat">
								<StatusIcon status={state} size="0.55em" />
								<span class="stat-num">{stateCounts[state]}</span>
								<span class="stat-label">{state}</span>
							</span>
						{/if}
					{/each}
				</div>
			</div>
			<div class="header-links">
				{#if autostartStatus?.installed}
					<a href="/settings" class="header-link" title="autostart settings">
						<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
							<path d="M8 2v4" /><path d="M5.2 4.2A5 5 0 1 0 10.8 4.2" />
						</svg>
					</a>
				{/if}
				<a href="/remote-control" class="header-link" title="remote control">
					<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
						<path d="M8 12v2" /><path d="M4.5 9.5a5 5 0 0 1 7 0" /><path d="M2 7a8 8 0 0 1 12 0" /><circle cx="8" cy="12" r="0.5" fill="currentColor" />
					</svg>
				</a>
			</div>
		</header>

		<div class="toolbar">
			<div class="toolbar-actions">
				<button
					class="toolbar-btn start"
					class:hidden={hasSelection
						? selectedStopped === 0
						: stoppedCount === 0}
					onclick={() =>
						hasSelection ? bulkAction("start") : actionAll("start")}
					disabled={bulkLoading}
					title={hasSelection
						? "Start selected (s)"
						: "Start all (s)"}
				>
					<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z" /></svg>
					<span class="toolbar-btn-label">{hasSelection ? "Start" : "Start all"}</span>
				</button>
				<button
					class="toolbar-btn stop"
					class:hidden={hasSelection
						? selectedRunning === 0
						: runningCount === 0}
					onclick={() =>
						hasSelection ? bulkAction("stop") : actionAll("stop")}
					disabled={bulkLoading}
					title={hasSelection ? "Stop selected (x)" : "Stop all (x)"}
				>
					<svg viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1.5" /></svg>
					<span class="toolbar-btn-label">{hasSelection ? "Stop" : "Stop all"}</span>
				</button>
				{#if hasSelection && selectedRunning > 0}
					<button
						class="toolbar-btn reload"
						onclick={() => bulkAction("reload")}
						disabled={bulkLoading}
						title="Reload selected (r)"
					>
						<svg
							viewBox="0 0 16 16"
							fill="none"
							stroke="currentColor"
							stroke-width="1.5"
							stroke-linecap="round"
						><path
								d="M2.5 8a5.5 5.5 0 0 1 9.9-3.2M13.5 8a5.5 5.5 0 0 1-9.9 3.2"
							/><polyline points="12 2 13 5 10 5.5" /><polyline
								points="4 14 3 11 6 10.5"
							/></svg
						>
						<span class="toolbar-btn-label">Reload</span>
					</button>
				{/if}
			</div>
			<label class="select-all">
				<input
					type="checkbox"
					bind:this={headerCheckbox}
					checked={allSelected}
					onclick={headerCheckClicked}
				/>
				{#if hasSelection}
					<span class="selection-count">{selectedNames.size} selected</span>
				{/if}
			</label>
		</div>

		<div class="service-list">
			{#each services as service (service.name)}
				<ServiceRow
					{service}
					onUpdate={refresh}
					selected={selectedNames.has(service.name)}
					onSelect={hasSelection ? toggleSelect : undefined}
				/>
			{/each}
		</div>

		{#if services.length === 0 && !error}
			<div class="empty">
				<p>No projects configured</p>
				<p class="empty-hint">
					Run <code>ky init</code> to get started
				</p>
			</div>
		{/if}
	</div>
</div>

<style>
	.page {
		--base: clamp(14px, 0.4rem + 0.8vw, 24px);
		font-size: var(--base);
		height: 100dvh;
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: 0.5em;
		overflow-y: auto;
	}

	.panel {
		width: 100%;
		max-width: 40em;
		display: flex;
		flex-direction: column;
	}

	.panel-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.4em 0.4em 0.6em;
		gap: 0.5em;
	}

	.header-left {
		display: flex;
		align-items: center;
		gap: 0.5em;
		flex-wrap: wrap;
		min-width: 0;
	}

	.logo {
		width: 1.4em;
		height: 1.4em;
		opacity: 0.4;
		filter: brightness(0) invert(1);
		flex-shrink: 0;
	}

	.brand-name {
		font-size: 1.1em;
		font-weight: 700;
		color: #555;
		letter-spacing: 0.02em;
	}

	.version {
		font-size: 0.65em;
		font-weight: 400;
		color: #444;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
	}

	.stats {
		display: flex;
		align-items: center;
		gap: 0.7em;
	}

	.stat {
		display: flex;
		align-items: center;
		gap: 0.25em;
		color: #555;
		font-size: 0.85em;
	}

	.stat-num {
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		font-weight: 600;
		color: #888;
	}

	.stat-label {
		color: #555;
	}

	.header-links {
		display: flex;
		align-items: center;
		gap: 0.3em;
		flex-shrink: 0;
	}

	.header-link {
		display: flex;
		align-items: center;
		color: #444;
		padding: 0.3em;
		border-radius: 0.3em;
		text-decoration: none;
		transition: color 0.15s;
	}

	.header-link:hover {
		color: #888;
	}

	.header-link svg {
		width: 1em;
		height: 1em;
	}

	.toolbar {
		display: flex;
		align-items: center;
		padding: 0.3em 0.4em;
		border-bottom: 1px solid #1e1e32;
		gap: 0.5em;
	}

	.toolbar-actions {
		display: flex;
		align-items: center;
		gap: 0.25em;
	}

	.toolbar-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.3em;
		border: none;
		background: #1a1a2e;
		color: #666;
		cursor: pointer;
		padding: 0.3em 0.6em;
		border-radius: 0.35em;
		font-size: 0.8em;
		font-weight: 500;
		transition: all 0.15s;
	}

	.toolbar-btn svg {
		width: 1.2em;
		height: 1.2em;
		flex-shrink: 0;
	}

	.toolbar-btn:hover {
		background: #252540;
		color: #bbb;
	}
	.toolbar-btn.start:hover { color: #55cc55; }
	.toolbar-btn.stop:hover { color: #dd6666; }
	.toolbar-btn.reload:hover { color: #7777cc; }
	.toolbar-btn:disabled { opacity: 0.3; cursor: not-allowed; }
	.toolbar-btn.hidden { display: none; }

	@media (max-width: 400px) {
		.toolbar-btn-label { display: none; }
		.toolbar-btn { padding: 0.35em; }
	}

	.select-all {
		display: flex;
		align-items: center;
		gap: 0.4em;
		cursor: pointer;
		font-size: 0.8em;
		color: #666;
		user-select: none;
		margin-left: auto;
	}

	.select-all input {
		width: 1em;
		height: 1em;
		accent-color: #6366f1;
		cursor: pointer;
		margin: 0;
	}

	.selection-count {
		color: #8888cc;
		font-weight: 500;
	}

	.service-list {
		display: flex;
		flex-direction: column;
	}

	.error-wrap {
		width: 100%;
		max-width: 40em;
		margin-bottom: 0.5em;
	}

	.error {
		background: #2a1010;
		border: 1px solid #442222;
		border-radius: 0.4em;
		padding: 0.6em 0.8em;
		color: #cc6666;
	}

	.error p {
		margin: 0.2em 0 0;
		font-size: 0.85em;
		color: #777;
	}

	.empty {
		padding: 2em 0;
		text-align: center;
	}

	.empty p {
		margin: 0;
		color: #555;
	}

	.empty-hint {
		margin-top: 0.4em !important;
		font-size: 0.9em !important;
		color: #444 !important;
	}

	code {
		background: #1a1a2e;
		padding: 0.1em 0.4em;
		border-radius: 0.2em;
		font-size: 0.9em;
	}
</style>
