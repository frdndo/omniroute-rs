// API client for the omniroute-rs admin + gateway APIs.
const BASE = (import.meta.env.VITE_PROXY_BASE as string) || "";

export function getAdminKey(): string {
  return localStorage.getItem("om_admin_key") || "";
}
export function setAdminKey(k: string) {
  localStorage.setItem("om_admin_key", k);
}
export function getGatewayKey(): string {
  return localStorage.getItem("om_gateway_key") || "";
}
export function setGatewayKey(k: string) {
  localStorage.setItem("om_gateway_key", k);
}

async function req<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(opts.headers as Record<string, string>),
  };
  if (path.startsWith("/admin") && getAdminKey()) {
    headers["Authorization"] = `Bearer ${getAdminKey()}`;
  }
  const res = await fetch(`${BASE}${path}`, { ...opts, headers });
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      msg = body?.error?.message || body?.message || msg;
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }
  return res.json() as Promise<T>;
}

export interface ProviderConnection {
  id: string;
  provider: string;
  name?: string;
  api_key?: string;
  is_active: boolean;
  priority: number;
  rate_limited_until?: string | null;
  backoff_level?: number;
}

export interface ApiKey {
  id: string;
  key: string;
  name?: string;
  is_active: boolean;
}

export interface Combo {
  id: string;
  name: string;
  kind: string;
  models: string[];
}

export interface LogEntry {
  ts: string;
  method: string;
  uri: string;
  status: number;
  duration_ms: number;
}

export const api = {
  health: () => req<any>("/health"),
  models: () => req<any>("/v1/models"),
  providers: {
    list: () => req<{ data: ProviderConnection[] }>("/admin/providers"),
    create: (body: any) => req<{ id: string }>("/admin/providers", { method: "POST", body: JSON.stringify(body) }),
    update: (id: string, body: any) => req<{ ok: boolean }>(`/admin/providers/${id}`, { method: "PUT", body: JSON.stringify(body) }),
    remove: (id: string) => req<{ ok: boolean }>(`/admin/providers/${id}`, { method: "DELETE" }),
  },
  keys: {
    list: () => req<{ data: ApiKey[] }>("/admin/api-keys"),
    create: (body: any) => req<{ id: string; key: string }>("/admin/api-keys", { method: "POST", body: JSON.stringify(body) }),
    update: (id: string, body: any) => req<{ ok: boolean }>(`/admin/api-keys/${id}`, { method: "PUT", body: JSON.stringify(body) }),
    remove: (id: string) => req<{ ok: boolean }>(`/admin/api-keys/${id}`, { method: "DELETE" }),
  },
  combos: {
    list: () => req<{ data: Combo[] }>("/admin/combos"),
    create: (body: any) => req<{ id: string }>("/admin/combos", { method: "POST", body: JSON.stringify(body) }),
    remove: (id: string) => req<{ ok: boolean }>(`/admin/combos/${id}`, { method: "DELETE" }),
  },
  logs: () => req<{ data: LogEntry[]; uptime_seconds: number }>("/admin/logs"),
  chat: (body: any, key?: string) => {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    const k = key || getGatewayKey();
    if (k) headers["Authorization"] = `Bearer ${k}`;
    return fetch(`${BASE}/v1/chat/completions`, { method: "POST", headers, body: JSON.stringify(body) });
  },
};
