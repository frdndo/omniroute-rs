import type { ReactNode } from "react";

export interface ModelOption {
  value: string;
  label: ReactNode;
}

export interface ModelGroup {
  label: string;
  options: ModelOption[];
}

/**
 * Group models by provider with provider info di label — parity playground
 * asli (label 'provider/model'). Model id duplikat (satu model di banyak
 * provider) di-dedupe: pilih owner yang TERKONFIGURASI dulu (sama kayak
 * resolver pool), fallback first-wins — jadi label = provider yang
 * kemungkinan besar dipakai routing.
 */
export function buildModelGroups(
  models: { id: string; owned_by?: string | null }[] | undefined,
  cfgSet: Set<string>
): ModelGroup[] {
  const ownerOf = new Map<string, string>();
  for (const m of models ?? []) {
    const owner = m.owned_by || "lainnya";
    const prev = ownerOf.get(m.id);
    if (!prev) ownerOf.set(m.id, owner);
    else if (!cfgSet.has(prev) && cfgSet.has(owner)) ownerOf.set(m.id, owner);
  }

  const byProvider = new Map<string, ModelOption[]>();
  for (const m of models ?? []) {
    const owner = m.owned_by || "lainnya";
    if (ownerOf.get(m.id) !== owner) continue; // tiap model cuma di 1 grup
    if (!byProvider.has(owner)) byProvider.set(owner, []);
    byProvider.get(owner)!.push({
      value: m.id,
      label: (
        <span style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
          <span style={{ fontFamily: "monospace" }}>{m.id}</span>
          <span style={{ color: "#999", fontSize: 11, flexShrink: 0 }}>
            {owner}
            {cfgSet.has(owner) ? " ⚙" : ""}
          </span>
        </span>
      ),
    });
  }

  return [...byProvider.entries()].map(([provider, options]) => ({
    label: `${provider} (${options.length})`,
    options,
  }));
}

/** filterOption untuk antd Select dengan label ReactNode (grup). */
export function modelFilterOption(input: string, opt: any): boolean {
  return String(opt?.value ?? "").toLowerCase().includes(input.toLowerCase());
}
