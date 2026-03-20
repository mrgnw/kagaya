<script lang="ts">
	import { onMount } from "svelte";
	import {
		getServices,
		getRemoteControl,
		enableRemoteControl,
		disableRemoteControl,
		updateRemoteControlMode,
		type ServiceInfo,
		type RemoteControlProject,
	} from "$lib/api";
	import StatusIcon from "$lib/components/StatusIcon.svelte";

	interface MergedProject {
		name: string;
		dir: string;
		rc: RemoteControlProject | null;
	}

	let services = $state<ServiceInfo[]>([]);
	let rcProjects = $state<RemoteControlProject[]>([]);
	let rcError = $state(false);
	let error = $state("");
	let refreshTimer: ReturnType<typeof setInterval>;
	let selectedNames = $state<Set<string>>(new Set());
	let bulkLoading = $state(false);
	let headerCheckbox = $state<HTMLInputElement | null>(null);
	let actionLoading = $state<Set<string>>(new Set());

	let merged = $derived.by(() => {
		const rcMap = new Map(rcProjects.map((r) => [r.name, r]));
		const result: MergedProject[] = services.map((s) => ({
			name: s.name,
			dir: s.dir,
			rc: rcMap.get(s.name) ?? null,
		}));
		for (const r of rcProjects) {
			if (!result.some((m) => m.name === r.name)) {
				result.push({ name: r.name, dir: r.dir, rc: r });
			}
		}
		return result;
	});

	let enabledCount = $derived(merged.filter((m) => m.rc).length);
	let runningCount = $derived(merged.filter((m) => m.rc?.running).length);

	let hasSelection = $derived(selectedNames.size > 0);
	let allSelected = $derived(merged.length > 0 && selectedNames.size === merged.length);
	let someSelected = $derived(hasSelection && !allSelected);

	let selectedEnabled = $derived(
		merged.filter((m) => selectedNames.has(m.name) && m.rc).length,
	);
	let selectedDisabled = $derived(
		merged.filter((m) => selectedNames.has(m.name) && !m.rc).length,
	);

	function syncIndeterminate() {
		if (headerCheckbox) {
			headerCheckbox.indeterminate = someSelected;
		}
	}

	async function refresh() {
		try {
			services = await getServices();
			error = "";
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
		try {
			rcProjects = await getRemoteControl();
			rcError = false;
		} catch {
			rcProjects = [];
			rcError = true;
		}
		selectedNames = new Set(
			[...selectedNames].filter((n) => merged.some((m) => m.name === n)),
		);
		queueMicrotask(syncIndeterminate);
	}

	function toggleSelect(name: string, checked: boolean) {
		const next = new Set(selectedNames);
		if (checked) next.add(name);
		else next.delete(name);
		selectedNames = next;
		queueMicrotask(syncIndeterminate);
	}

	function headerCheckClicked() {
		if (allSelected || someSelected) {
			selectedNames = new Set();
		} else {
			selectedNames = new Set(merged.map((m) => m.name));
		}
		queueMicrotask(syncIndeterminate);
	}

	async function toggleRC(project: MergedProject) {
		const loading = new Set(actionLoading);
		loading.add(project.name);
		actionLoading = loading;
		try {
			if (project.rc) {
				await disableRemoteControl(project.name);
			} else {
				await enableRemoteControl(project.name, project.dir, "same-dir");
			}
			setTimeout(refresh, 300);
		} catch (e) {
			console.error(e);
		} finally {
			const done = new Set(actionLoading);
			done.delete(project.name);
			actionLoading = done;
		}
	}

	async function changeMode(name: string, mode: string) {
		const loading = new Set(actionLoading);
		loading.add(name);
		actionLoading = loading;
		try {
			await updateRemoteControlMode(name, mode);
			setTimeout(refresh, 300);
		} catch (e) {
			console.error(e);
		} finally {
			const done = new Set(actionLoading);
			done.delete(name);
			actionLoading = done;
		}
	}

	async function bulkEnable() {
		bulkLoading = true;
		const targets = merged.filter((m) => selectedNames.has(m.name) && !m.rc);
		try {
			await Promise.allSettled(
				targets.map((m) => enableRemoteControl(m.name, m.dir, "same-dir")),
			);
			setTimeout(refresh, 300);
		} finally {
			bulkLoading = false;
		}
	}

	async function bulkDisable() {
		bulkLoading = true;
		const targets = merged.filter((m) => selectedNames.has(m.name) && m.rc);
		try {
			await Promise.allSettled(
				targets.map((m) => disableRemoteControl(m.name)),
			);
			setTimeout(refresh, 300);
		} finally {
			bulkLoading = false;
		}
	}

	onMount(() => {
		refresh();
		refreshTimer = setInterval(refresh, 5000);
		return () => clearInterval(refreshTimer);
	});
</script>

<div class="page">
	{#if error}
		<div class="error-wrap">
			<div class="error">
				{error}
				<p>Make sure the kagaya server is running on port 13369</p>
			</div>
		</div>
	{/if}

	{#if rcError && !error}
		<div class="error-wrap">
			<div class="warning">
				claude-rc daemon not reachable
				<p>Remote control status may be unavailable</p>
			</div>
		</div>
	{/if}

	<div class="panel">
		<header class="panel-header">
			<div class="title-row">
				<a href="/" class="back" title="Back to dashboard">
					<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
						<path d="M10 3L5 8l5 5" />
					</svg>
				</a>
				<h1>Remote Control</h1>
			</div>
			<div class="stats">
				{#if runningCount > 0}
					<span class="stat">
						<StatusIcon status="running" size="0.6em" />
						<span class="stat-num">{runningCount}</span>
						<span class="stat-label">running</span>
					</span>
				{/if}
				<span class="stat">
					<span class="stat-num">{enabledCount}</span>
					<span class="stat-label">/ {merged.length} enabled</span>
				</span>
			</div>
		</header>

		<div class="toolbar">
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
			<div class="toolbar-actions">
				{#if hasSelection && selectedDisabled > 0}
					<button
						class="toolbar-btn start"
						onclick={bulkEnable}
						disabled={bulkLoading}
						title="Enable selected"
					>
						<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5v11l9-5.5z" /></svg>
						<span class="toolbar-btn-label">Enable</span>
					</button>
				{/if}
				{#if hasSelection && selectedEnabled > 0}
					<button
						class="toolbar-btn stop"
						onclick={bulkDisable}
						disabled={bulkLoading}
						title="Disable selected"
					>
						<svg viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1.5" /></svg>
						<span class="toolbar-btn-label">Disable</span>
					</button>
				{/if}
			</div>
		</div>

		<div class="project-list">
			{#each merged as project (project.name)}
				{@const enabled = !!project.rc}
				{@const loading = actionLoading.has(project.name)}
				<div class="row" class:dimmed={!enabled}>
					<label class="row-check">
						<input
							type="checkbox"
							checked={selectedNames.has(project.name)}
							onchange={(e) => toggleSelect(project.name, e.currentTarget.checked)}
						/>
					</label>
					<span class="row-status">
						{#if loading}
							<StatusIcon status="running" size="0.7em" loading={true} />
						{:else if enabled && project.rc?.running}
							<StatusIcon status="running" size="0.7em" />
						{:else if enabled}
							<StatusIcon status="stopped" size="0.7em" />
						{:else}
							<span class="dot-placeholder"></span>
						{/if}
					</span>
					<span class="row-name">{project.name}</span>
					<span class="row-dir">{project.dir}</span>
					{#if enabled}
						<select
							class="row-mode"
							value={project.rc?.mode ?? "same-dir"}
							onchange={(e) => changeMode(project.name, e.currentTarget.value)}
							disabled={loading}
						>
							<option value="same-dir">same-dir</option>
							<option value="worktree">worktree</option>
							<option value="session">session</option>
						</select>
					{:else}
						<span class="row-mode-placeholder"></span>
					{/if}
					<button
						class="toggle-btn"
						class:enabled
						onclick={() => toggleRC(project)}
						disabled={loading}
					>
						{enabled ? "Disable" : "Enable"}
					</button>
				</div>
			{/each}
		</div>

		{#if merged.length === 0 && !error}
			<div class="empty">
				<p>No projects found</p>
			</div>
		{/if}
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
		justify-content: space-between;
		padding: 0 0.6em 1em;
		gap: 1em;
		flex-wrap: wrap;
	}

	.title-row {
		display: flex;
		align-items: center;
		gap: 0.5em;
	}

	.back {
		display: inline-flex;
		align-items: center;
		color: #555;
		transition: color 0.15s;
	}

	.back:hover {
		color: #999;
	}

	.back svg {
		width: 1.2em;
		height: 1.2em;
	}

	h1 {
		margin: 0;
		font-size: 1.3em;
		font-weight: 700;
		color: #555;
		letter-spacing: 0.02em;
	}

	.stats {
		display: flex;
		align-items: center;
		gap: 1em;
	}

	.stat {
		display: flex;
		align-items: center;
		gap: 0.4em;
		color: #555;
	}

	.stat-num {
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		font-weight: 600;
		color: #888;
	}

	.stat-label {
		color: #555;
		font-size: 0.85em;
	}

	.toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.5em 0.6em;
		border-bottom: 1px solid #1e1e32;
		gap: 0.5em;
		flex-wrap: wrap;
	}

	.select-all {
		display: flex;
		align-items: center;
		gap: 0.5em;
		cursor: pointer;
		font-size: 0.85em;
		color: #666;
		user-select: none;
	}

	.select-all input {
		width: 1.1em;
		height: 1.1em;
		accent-color: #6366f1;
		cursor: pointer;
		margin: 0;
	}

	.selection-count {
		color: #8888cc;
		font-weight: 500;
	}

	.toolbar-actions {
		display: flex;
		align-items: center;
		gap: 0.35em;
	}

	.toolbar-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.4em;
		border: none;
		background: #1a1a2e;
		color: #666;
		cursor: pointer;
		padding: 0.35em 0.75em;
		border-radius: 0.4em;
		font-size: 0.85em;
		font-weight: 500;
		transition: all 0.15s;
	}

	.toolbar-btn svg {
		width: 1.4em;
		height: 1.4em;
		flex-shrink: 0;
	}

	.toolbar-btn:hover {
		background: #252540;
		color: #bbb;
	}

	.toolbar-btn.start:hover {
		color: #55cc55;
	}

	.toolbar-btn.stop:hover {
		color: #dd6666;
	}

	.toolbar-btn:disabled {
		opacity: 0.3;
		cursor: not-allowed;
	}

	@media (max-width: 400px) {
		.toolbar-btn-label {
			display: none;
		}
		.toolbar-btn {
			padding: 0.4em;
		}
	}

	.project-list {
		display: flex;
		flex-direction: column;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 0.5em;
		padding: 0.5em 0.6em;
		border-bottom: 1px solid #1a1a2e;
		transition: opacity 0.15s;
	}

	.row.dimmed {
		opacity: 0.5;
	}

	.row:hover {
		background: #13132a;
	}

	.row-check {
		display: flex;
		align-items: center;
		cursor: pointer;
	}

	.row-check input {
		width: 1.1em;
		height: 1.1em;
		accent-color: #6366f1;
		cursor: pointer;
		margin: 0;
	}

	.row-status {
		display: flex;
		align-items: center;
		width: 1.2em;
		flex-shrink: 0;
	}

	.dot-placeholder {
		width: 1.2em;
	}

	.row-name {
		font-weight: 600;
		color: #ccc;
		white-space: nowrap;
		min-width: 0;
	}

	.row-dir {
		color: #444;
		font-size: 0.8em;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		flex: 1;
		min-width: 0;
	}

	.row-mode {
		background: #1a1a2e;
		color: #999;
		border: 1px solid #1e1e32;
		border-radius: 0.3em;
		padding: 0.2em 0.4em;
		font-size: 0.8em;
		font-family: inherit;
		cursor: pointer;
		flex-shrink: 0;
	}

	.row-mode:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.row-mode option {
		background: #1a1a2e;
		color: #999;
	}

	.row-mode-placeholder {
		width: 5.5em;
		flex-shrink: 0;
	}

	.toggle-btn {
		border: 1px solid #1e1e32;
		background: #1a1a2e;
		color: #666;
		cursor: pointer;
		padding: 0.2em 0.6em;
		border-radius: 0.3em;
		font-size: 0.8em;
		font-family: inherit;
		font-weight: 500;
		transition: all 0.15s;
		flex-shrink: 0;
	}

	.toggle-btn:hover {
		background: #252540;
		color: #bbb;
	}

	.toggle-btn.enabled:hover {
		color: #dd6666;
		border-color: #442222;
	}

	.toggle-btn:not(.enabled):hover {
		color: #55cc55;
		border-color: #224422;
	}

	.toggle-btn:disabled {
		opacity: 0.3;
		cursor: not-allowed;
	}

	.error-wrap {
		width: 100%;
		max-width: 42em;
		margin-bottom: 1em;
	}

	.error {
		background: #2a1010;
		border: 1px solid #442222;
		border-radius: 0.5em;
		padding: 0.8em 1em;
		color: #cc6666;
	}

	.error p {
		margin: 0.3em 0 0;
		font-size: 0.85em;
		color: #777;
	}

	.warning {
		background: #1a1a10;
		border: 1px solid #333322;
		border-radius: 0.5em;
		padding: 0.8em 1em;
		color: #aa9944;
	}

	.warning p {
		margin: 0.3em 0 0;
		font-size: 0.85em;
		color: #777;
	}

	.empty {
		padding: 3em 0;
		text-align: center;
	}

	.empty p {
		margin: 0;
		color: #555;
	}

	@media (min-width: 1200px) {
		.panel-header {
			flex-direction: column;
			align-items: center;
			gap: 0.6em;
			padding-bottom: 1.4em;
		}
	}
</style>
