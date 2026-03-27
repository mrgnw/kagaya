const API_PORT = 13369;

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

function apiBase(): string {
  if (typeof window === "undefined") return `http://localhost:${API_PORT}`;
  return "";
}

async function tauriInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function httpGet<T>(path: string): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`);
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  return res.json();
}

async function httpPost<T>(path: string): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, { method: "POST" });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  return res.json();
}

async function httpPostJson<T>(path: string, data: unknown): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  return res.json();
}

async function httpPatchJson<T>(path: string, data: unknown): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  return res.json();
}

async function httpDelete<T>(path: string): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, { method: "DELETE" });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  return res.json();
}

async function httpGetText(path: string): Promise<string> {
  const res = await fetch(`${apiBase()}${path}`);
  if (!res.ok) throw new Error(res.statusText);
  return res.text();
}

export interface ServiceInfo {
  name: string;
  dir: string;
  running: boolean;
  state: "on" | "degraded" | "err" | "off";
  autostart: boolean;
  error_tail?: string[];
}

export interface AutostartStatus {
  installed: boolean;
  active: boolean;
  agent_path: string | null;
  projects: string[];
}

export interface ProcessInfo {
  name: string;
  pid: number | null;
  status: string;
  autostart: boolean;
  ports: number[];
  cpu_percent?: number;
  memory_bytes?: number;
}

export interface ServiceDetail {
  name: string;
  dir: string;
  running: boolean;
  state: "on" | "degraded" | "err" | "off";
  processes: ProcessInfo[];
}

export interface TmuxPane {
  session: string;
  window: number;
  pane: number;
  command: string;
  pid: number;
}

export async function getVersion(): Promise<string> {
  const res = await httpGet<{ version: string }>("/api/version");
  return res.version;
}

export async function getServices(): Promise<ServiceInfo[]> {
  if (isTauri()) return tauriInvoke("get_services");
  return httpGet("/api/services");
}

export async function getServiceDetail(name: string): Promise<ServiceDetail> {
  if (isTauri()) return tauriInvoke("get_service_detail", { name });
  return httpGet(`/api/services/${name}`);
}

export async function startService(name: string): Promise<string> {
  if (isTauri()) return tauriInvoke("start_service", { name });
  const res = await httpPost<{ message: string }>(
    `/api/services/${name}/start`,
  );
  return res.message;
}

export async function stopService(name: string): Promise<string> {
  if (isTauri()) return tauriInvoke("stop_service", { name });
  const res = await httpPost<{ message: string }>(`/api/services/${name}/stop`);
  return res.message;
}

export async function reloadService(name: string): Promise<string> {
  if (isTauri()) return tauriInvoke("reload_service", { name });
  const res = await httpPost<{ message: string }>(
    `/api/services/${name}/reload`,
  );
  return res.message;
}

export async function echoService(name: string): Promise<string> {
  if (isTauri()) return tauriInvoke("echo_service", { name });
  return httpGetText(`/api/services/${name}/echo`);
}

export async function getPanes(name: string): Promise<TmuxPane[]> {
  if (isTauri()) return tauriInvoke("get_panes", { name });
  return httpGet(`/api/services/${name}/panes`);
}

export async function capturePane(
  name: string,
  window: number,
  pane: number,
): Promise<string> {
  if (isTauri()) return tauriInvoke("capture_pane", { name, window, pane });
  return httpGetText(`/api/services/${name}/panes/${window}/${pane}`);
}

export async function restartProcess(
  name: string,
  process: string,
): Promise<string> {
  if (isTauri()) return tauriInvoke("restart_process", { name, process });
  const res = await httpPost<{ message: string }>(
    `/api/services/${name}/processes/${process}/restart`,
  );
  return res.message;
}

export async function killProcess(
  name: string,
  process: string,
): Promise<string> {
  if (isTauri()) return tauriInvoke("kill_process", { name, process });
  const res = await httpPost<{ message: string }>(
    `/api/services/${name}/processes/${process}/kill`,
  );
  return res.message;
}

export function echoWebSocketUrl(name: string): string {
  if (typeof window === "undefined")
    return `ws://localhost:${API_PORT}/ws/echo/${name}`;
  const wsProto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${wsProto}//${window.location.host}/ws/echo/${name}`;
}

export interface RemoteControlProject {
  name: string;
  dir: string;
  mode: string;
  running: boolean;
  pid: number | null;
}

export async function getRemoteControl(): Promise<RemoteControlProject[]> {
  return httpGet("/api/remote-control");
}

export async function enableRemoteControl(name: string, dir: string, mode: string): Promise<string> {
  const res = await httpPostJson<{ message: string }>(`/api/remote-control/${name}`, { dir, mode });
  return res.message;
}

export async function disableRemoteControl(name: string): Promise<string> {
  const res = await httpDelete<{ message: string }>(`/api/remote-control/${name}`);
  return res.message;
}

export async function updateRemoteControlMode(name: string, mode: string): Promise<string> {
  const res = await httpPatchJson<{ message: string }>(`/api/remote-control/${name}`, { mode });
  return res.message;
}

export interface DirEntry {
  name: string;
  path: string;
  children: DirEntry[];
}

export async function getDevDirs(): Promise<DirEntry[]> {
  return httpGet("/api/dev-dirs");
}

export async function getAutostartStatus(): Promise<AutostartStatus> {
  return httpGet("/api/autostart");
}

export async function enableAutostart(): Promise<string> {
  const res = await httpPost<{ message: string }>("/api/autostart/on");
  return res.message;
}

export async function disableAutostart(): Promise<string> {
  const res = await httpPost<{ message: string }>("/api/autostart/off");
  return res.message;
}
