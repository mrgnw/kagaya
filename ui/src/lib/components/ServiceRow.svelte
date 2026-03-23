<script lang="ts">
	import { SvelteSet } from "svelte/reactivity";
	import type { ServiceInfo } from "$lib/api";
	import {
		startService,
		stopService,
		reloadService,
		getServiceDetail,
		restartProcess,
		killProcess,
		type ProcessInfo,
	} from "$lib/api";
	import StatusIcon from "./StatusIcon.svelte";

	type AggregateState = "on" | "degraded" | "err" | "off";
	type ServiceWithState = ServiceInfo & { state: AggregateState };

	let {
		service,
		onUpdate,
		selected = false,
		onSelect,
	}: {
		service: ServiceWithState;
		onUpdate: () => void;
		selected?: boolean;
		onSelect?: (name: string, checked: boolean) => void;
	} = $props();

	let loading = $state(false);
	let expanded = $state(false);
	let processes = $state<ProcessInfo[]>([]);
	let detailLoading = $state(false);
	let detailLoaded = $state(false);
	let procLoading = new SvelteSet<string>();

	async function handleAction(e: Event, action: "start" | "stop" | "reload") {
		e.preventDefault();
		e.stopPropagation();
		loading = true;
		try {
			if (action === "start") await startService(service.name);
			else if (action === "stop") await stopService(service.name);
			else await reloadService(service.name);
			setTimeout(() => {
				onUpdate();
				if (expanded) fetchDetail();
			}, 300);
		} catch (e) {
			console.error(e);
		} finally {
			loading = false;
		}
	}

	async function fetchDetail() {
		detailLoading = true;
		try {
			const detail = await getServiceDetail(service.name);
			processes = detail.processes;
			detailLoaded = true;
		} catch (e) {
			console.error(e);
		} finally {
			detailLoading = false;
		}
	}

	function toggleExpand(e: Event) {
		e.preventDefault();
		expanded = !expanded;
		if (expanded && !detailLoaded) {
			fetchDetail();
		}
	}

	async function handleProcessAction(
		procName: string,
		action: "restart" | "kill",
	) {
		procLoading.add(procName);
		try {
			if (action === "restart")
				await restartProcess(service.name, procName);
			else await killProcess(service.name, procName);
			setTimeout(fetchDetail, 400);
		} catch (e) {
			console.error(e);
		} finally {
			procLoading.delete(procName);
		}
	}

	function isRunning(status: string): boolean {
		return status.startsWith("running");
	}

	function statusLabel(status: string): string {
		if (status.startsWith("running")) return "running";
		if (status.startsWith("crashed")) return "crashed";
		if (status.startsWith("failed")) return "failed";
		return status;
	}

	function statusDetail(status: string): string {
		const m = status.match(/\((.+)\)/);
		return m ? m[1] : "";
	}

	let aggregateState = $derived(service.state);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="service-block" class:expanded>
	<a
		href="/service/{service.name}"
		class="row"
		class:selected
		class:stopped={aggregateState === "off"}
		onclick={toggleExpand}
	>
		<span class="actions" onclick={(e: MouseEvent) => e.preventDefault()}>
			<button
				class="btn"
				class:start={!service.running}
				class:stop={service.running}
				onclick={(e) =>
					handleAction(e, service.running ? "stop" : "start")}
				disabled={loading}
				title={service.running ? "Stop" : "Start"}
			>
				{#if service.running}
					<svg viewBox="0 0 16 16" fill="currentColor"
						><rect
							x="3"
							y="3"
							width="10"
							height="10"
							rx="1.5"
						/></svg
					>
				{:else}
					<svg viewBox="0 0 16 16" fill="currentColor"
						><path d="M4 2.5v11l9-5.5z" /></svg
					>
				{/if}
			</button>

			<button
				class="btn reload"
				class:ghost={!service.running}
				onclick={(e) => handleAction(e, "reload")}
				disabled={loading || !service.running}
				title="Reload"
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
			</button>

		</span>

		<span class="right">
			{#if onSelect}
				<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<label
					class="check-wrap"
					onclick={(e: MouseEvent) => e.stopPropagation()}
				>
					<input
						type="checkbox"
						checked={selected}
						onchange={(e) =>
							onSelect?.(
								service.name,
								(e.target as HTMLInputElement).checked,
							)}
					/>
				</label>
			{:else}
				<StatusIcon
					status={aggregateState}
					{loading}
					size="0.6em"
				/>
			{/if}
			<span class="name">{service.name}</span>
			{#if service.autostart}
				<span class="autostart-badge" title="starts on login">
					<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
						<path d="M8 2v4" /><path d="M5.2 4.2A5 5 0 1 0 10.8 4.2" />
					</svg>
				</span>
			{/if}
			{#if service.running}
				<span class="chevron" class:open={expanded}>
					<svg viewBox="0 0 16 16" fill="currentColor"
						><path d="M6 4l4 4-4 4z" /></svg
					>
				</span>
			{/if}
		</span>
	</a>

	{#if expanded && service.running}
		<div class="sub-processes">
			{#if detailLoading && !detailLoaded}
				<div class="sub-row">
					<span class="sub-left">
						<StatusIcon status="stopped" loading size="0.55em" />
						<span class="sub-name dim">loading...</span>
					</span>
				</div>
			{:else if processes.length === 0}
				<div class="sub-row">
					<span class="sub-left">
						<span class="sub-name dim">no processes</span>
					</span>
				</div>
			{:else}
				{#each processes as proc (proc.name)}
					<div class="sub-row">
						<span class="sub-left">
							<StatusIcon status={proc.status} size="0.55em" />
							<span class="sub-name">{proc.name}</span>
							<span class="sub-badge {statusLabel(proc.status)}">{statusLabel(proc.status)}</span>
							{#if statusDetail(proc.status)}
								<span class="sub-detail">{statusDetail(proc.status)}</span>
							{/if}
							{#if proc.ports?.length}
								<span class="sub-ports">{proc.ports.map(p => `:${p}`).join(', ')}</span>
							{/if}
						</span>
						<span class="sub-actions">
							{#if proc.pid}
								<span class="sub-pid">pid {proc.pid}</span>
							{/if}
							{#if isRunning(proc.status)}
								<button
									class="sub-btn stop"
									title="Kill {proc.name}"
									disabled={procLoading.has(proc.name)}
									onclick={() =>
										handleProcessAction(proc.name, "kill")}
								>
									<svg viewBox="0 0 16 16" fill="currentColor"
										><rect
											x="3"
											y="3"
											width="10"
											height="10"
											rx="1.5"
										/></svg
									>
								</button>
								<button
									class="sub-btn reload"
									title="Restart {proc.name}"
									disabled={procLoading.has(proc.name)}
									onclick={() =>
										handleProcessAction(
											proc.name,
											"restart",
										)}
								>
									<svg
										viewBox="0 0 16 16"
										fill="none"
										stroke="currentColor"
										stroke-width="1.5"
										stroke-linecap="round"
										><path
											d="M2.5 8a5.5 5.5 0 0 1 9.9-3.2M13.5 8a5.5 5.5 0 0 1-9.9 3.2"
										/><polyline
											points="12 2 13 5 10 5.5"
										/><polyline
											points="4 14 3 11 6 10.5"
										/></svg
									>
								</button>
							{:else}
								<button
									class="sub-btn start"
									title="Restart {proc.name}"
									disabled={procLoading.has(proc.name)}
									onclick={() =>
										handleProcessAction(
											proc.name,
											"restart",
										)}
								>
									<svg viewBox="0 0 16 16" fill="currentColor"
										><path d="M4 2.5v11l9-5.5z" /></svg
									>
								</button>
							{/if}
						</span>
					</div>
				{/each}
			{/if}
		</div>
	{/if}
</div>

<style>
	.service-block {
		display: flex;
		flex-direction: column;
		border-bottom: 1px solid #1a1a2a;
	}

	.service-block:last-child {
		border-bottom: none;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 0.3em;
		padding: 0.4em;
		text-decoration: none;
		transition: background 0.1s;
	}

	.row:hover {
		background: #1a1a3066;
	}

	.row.selected {
		background: #1e1e4033;
	}

	.right {
		display: flex;
		align-items: center;
		gap: 0.5em;
		min-width: 0;
		flex: 1;
	}

	.check-wrap {
		display: flex;
		align-items: center;
		justify-content: center;
		cursor: pointer;
		flex-shrink: 0;
	}

	.check-wrap input {
		width: 1.1em;
		height: 1.1em;
		accent-color: #6366f1;
		cursor: pointer;
		margin: 0;
	}

	.name {
		font-size: 1em;
		font-weight: 600;
		color: #ccc;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.row:hover .name {
		color: #eee;
	}

	.autostart-badge {
		display: flex;
		align-items: center;
		color: #555;
		flex-shrink: 0;
	}

	.autostart-badge svg {
		width: 0.85em;
		height: 0.85em;
	}

	.chevron {
		display: flex;
		align-items: center;
		color: #444;
		transition: transform 0.15s ease;
		flex-shrink: 0;
	}

	.chevron svg {
		width: 1em;
		height: 1em;
	}

	.chevron.open {
		transform: rotate(90deg);
	}

	.row:hover .chevron {
		color: #666;
	}

	.actions {
		display: grid;
		grid-template-columns: 2em 2em;
		gap: 0.1em;
		flex-shrink: 0;
	}

	.btn {
		width: 2em;
		height: 2em;
		border: none;
		background: none;
		color: #444;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		border-radius: 0.4em;
		transition: all 0.12s;
	}

	.btn svg {
		width: 1.3em;
		height: 1.3em;
	}

	.btn:hover {
		color: #ccc;
		background: #252540;
	}
	.btn.start:hover {
		color: #55cc55;
	}
	.btn.stop:hover {
		color: #dd6666;
	}
	.btn.reload:hover {
		color: #7777cc;
	}
	.btn:disabled {
		opacity: 0.2;
		cursor: default;
	}
	.btn:disabled:hover {
		background: none;
		color: #444;
	}
	.btn.ghost {
		opacity: 0;
		pointer-events: none;
	}

	/* -- Sub-processes -- */
	.sub-processes {
		padding: 0 0.6em 0.8em;
		padding-left: 2.2em;
		display: flex;
		flex-direction: column;
		gap: 0.15em;
	}

	.sub-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.4em 0.8em;
		border-radius: 0.4em;
		background: #14142a;
	}

	.sub-left {
		display: flex;
		align-items: center;
		gap: 0.5em;
		min-width: 0;
	}

	.sub-name {
		font-size: 0.9em;
		font-weight: 500;
		color: #aaa;
	}

	.sub-name.dim {
		color: #555;
		font-style: italic;
	}

	.sub-badge {
		font-size: 0.7em;
		font-weight: 500;
		padding: 0.15em 0.5em;
		border-radius: 0.3em;
		text-transform: uppercase;
		letter-spacing: 0.03em;
	}
	.sub-badge.running {
		color: #44bb44;
		background: #44bb4415;
	}
	.sub-badge.stopped {
		color: #666;
		background: #66666615;
	}
	.sub-badge.crashed {
		color: #ccaa44;
		background: #ccaa4418;
	}
	.sub-badge.failed {
		color: #cc4444;
		background: #cc444418;
	}

	.sub-detail {
		font-size: 0.75em;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		color: #555;
	}

	.sub-ports {
		font-size: 0.75em;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		color: #6688aa;
	}

	.sub-actions {
		display: flex;
		align-items: center;
		gap: 0.3em;
		flex-shrink: 0;
	}

	.sub-pid {
		font-size: 0.7em;
		font-family: "SF Mono", Menlo, Monaco, "Courier New", monospace;
		color: #444;
		margin-right: 0.3em;
	}

	.sub-btn {
		width: 2em;
		height: 2em;
		border: none;
		background: none;
		color: #444;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		border-radius: 0.4em;
		transition: all 0.12s;
	}

	.sub-btn svg {
		width: 1.2em;
		height: 1.2em;
	}

	.sub-btn:hover {
		background: #252540;
		color: #ccc;
	}
	.sub-btn.start:hover {
		color: #55cc55;
	}
	.sub-btn.stop:hover {
		color: #dd6666;
	}
	.sub-btn.reload:hover {
		color: #7777cc;
	}
	.sub-btn:disabled {
		opacity: 0.25;
		cursor: default;
	}
	.sub-btn:disabled:hover {
		background: none;
		color: #444;
	}
</style>
