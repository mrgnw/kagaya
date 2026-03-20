<script lang="ts">
    import { onMount } from "svelte";
    import {
        getServices,
        getAutostartStatus,
        startService,
        stopService,
        reloadService,
        type ServiceInfo,
        type AutostartStatus,
    } from "$lib/api";
    import ServiceRow from "$lib/components/ServiceRow.svelte";
    import StatusIcon from "$lib/components/StatusIcon.svelte";
    import logoSvg from "$lib/assets/logo.svg";

    let services = $state<ServiceInfo[]>([]);
    let autostartStatus = $state<AutostartStatus | null>(null);
    let error = $state("");
    let refreshTimer: ReturnType<typeof setInterval>;
    let selectedNames = $state<Set<string>>(new Set());
    let bulkLoading = $state(false);
    let headerCheckbox = $state<HTMLInputElement | null>(null);

    let hasSelection = $derived(selectedNames.size > 0);
    let allSelected = $derived(
        services.length > 0 && selectedNames.size === services.length,
    );
    let someSelected = $derived(hasSelection && !allSelected);
    let runningCount = $derived(services.filter((s) => s.running).length);
    let stoppedCount = $derived(services.filter((s) => !s.running).length);

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
            services = await getServices();
            error = "";
            selectedNames = new Set(
                [...selectedNames].filter((n) =>
                    services.some((s) => s.name === n),
                ),
            );
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
            selectedNames = new Set(services.map((s) => s.name));
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
                selectedNames = new Set();
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
        <!-- Header -->
        <header class="panel-header">
            <div class="brand">
                <img src={logoSvg} alt="" class="logo" />
                <span class="brand-name">kagaya</span>
            </div>
            <div class="stats" aria-label="Project status">
                {#if runningCount > 0}
                    <span class="stat">
                        <StatusIcon status="running" size="0.6em" />
                        <span class="stat-num">{runningCount}</span>
                        <span class="stat-label">running</span>
                    </span>
                {/if}
                {#if stoppedCount > 0}
                    <span class="stat">
                        <StatusIcon status="stopped" size="0.6em" />
                        <span class="stat-num">{stoppedCount}</span>
                        <span class="stat-label">stopped</span>
                    </span>
                {/if}
                {#if autostartStatus?.installed}
                    <a href="/settings" class="stat autostart-stat" title="autostart settings">
                        <svg class="autostart-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                            <path d="M8 2v4" /><path d="M5.2 4.2A5 5 0 1 0 10.8 4.2" />
                        </svg>
                        <span class="stat-label">autostart</span>
                    </a>
                {/if}
                <a href="/remote-control" class="stat autostart-stat" title="remote control">
                    <svg class="autostart-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
                        <path d="M8 12v2" /><path d="M4.5 9.5a5 5 0 0 1 7 0" /><path d="M2 7a8 8 0 0 1 12 0" /><circle cx="8" cy="12" r="0.5" fill="currentColor" />
                    </svg>
                    <span class="stat-label">remote</span>
                </a>
            </div>
        </header>

        <!-- Toolbar -->
        <div class="toolbar">
            <label class="select-all">
                <input
                    type="checkbox"
                    bind:this={headerCheckbox}
                    checked={allSelected}
                    onclick={headerCheckClicked}
                />
                {#if hasSelection}
                    <span class="selection-count"
                        >{selectedNames.size} selected</span
                    >
                {/if}
            </label>
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
                    <svg viewBox="0 0 16 16" fill="currentColor"
                        ><path d="M4 2.5v11l9-5.5z" /></svg
                    >
                    <span class="toolbar-btn-label"
                        >{hasSelection ? "Start" : "Start all"}</span
                    >
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
                    <svg viewBox="0 0 16 16" fill="currentColor"
                        ><rect
                            x="3"
                            y="3"
                            width="10"
                            height="10"
                            rx="1.5"
                        /></svg
                    >
                    <span class="toolbar-btn-label"
                        >{hasSelection ? "Stop" : "Stop all"}</span
                    >
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
        </div>

        <!-- Service list -->
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
    /*
	 * Fluid scale:  at 400px vw → 14px base,  at 2400px+ vw → 32px base
	 * Everything uses em so it all scales together.
	 */
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

    /* ── Header ── */
    .panel-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0 0.6em 1em;
        gap: 1em;
        flex-wrap: wrap;
    }

    .brand {
        display: flex;
        align-items: center;
        gap: 0.6em;
    }

    .logo {
        width: 1.8em;
        height: 1.8em;
        opacity: 0.45;
        filter: brightness(0) invert(1);
    }

    .brand-name {
        font-size: 1.3em;
        font-weight: 700;
        color: #555;
        letter-spacing: 0.02em;
    }

    /* At large viewports, stack logo above brand name centered */
    @media (min-width: 1200px) {
        .panel-header {
            flex-direction: column;
            align-items: center;
            gap: 0.6em;
            padding-bottom: 1.4em;
        }

        .brand {
            flex-direction: column;
            align-items: center;
            gap: 0.3em;
        }

        .logo {
            width: 3.5em;
            height: 3.5em;
            opacity: 0.5;
        }

        .brand-name {
            font-size: 1.5em;
        }

        .stats {
            gap: 1.4em;
        }
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

    .autostart-stat {
        text-decoration: none;
        transition: color 0.15s;
    }

    .autostart-stat:hover {
        color: #888;
    }

    .autostart-stat:hover .stat-label {
        color: #888;
    }

    .autostart-icon {
        width: 0.85em;
        height: 0.85em;
        color: #555;
    }

    /* ── Toolbar ── */
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
    .toolbar-btn.reload:hover {
        color: #7777cc;
    }
    .toolbar-btn:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }
    .toolbar-btn.hidden {
        display: none;
    }

    /* Hide button text at very narrow widths, keep icons */
    @media (max-width: 400px) {
        .toolbar-btn-label {
            display: none;
        }
        .toolbar-btn {
            padding: 0.4em;
        }
    }

    /* ── Service list ── */
    .service-list {
        display: flex;
        flex-direction: column;
    }

    /* ── Error ── */
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

    /* ── Empty ── */
    .empty {
        padding: 3em 0;
        text-align: center;
    }

    .empty p {
        margin: 0;
        color: #555;
    }

    .empty-hint {
        margin-top: 0.5em !important;
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
