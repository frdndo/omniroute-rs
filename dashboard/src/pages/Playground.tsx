import { useState, useMemo } from "react";
import { Card, Input, Button, Select, Space, Switch, Alert } from "antd";
import { useQuery } from "@tanstack/react-query";
import { api, getGatewayKey, setGatewayKey } from "../api/client";

export default function Playground() {
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const combos = useQuery({ queryKey: ["combos"], queryFn: api.combos.list });
  const [model, setModel] = useState<string>("gpt-4o");
  const [prompt, setPrompt] = useState("");
  const [stream, setStream] = useState(false);
  const [loading, setLoading] = useState(false);
  const [output, setOutput] = useState("");
  const [gwKey, setGwKey] = useState(getGatewayKey());

  // Grup per provider (owned_by) — parity playground asli yang filter
  // model per provider. Combos juga bisa dipilih sebagai "model".
  // Model id DUPLIKAT (satu model di banyak provider, mis. deepseek-v4-pro
  // di 5+ provider) di-dedupe first-wins — kalau tidak, antd Select
  // nyangkut (banyak option value sama).
  const grouped = useMemo(() => {
    const byProvider = new Map<string, { value: string; label: string }[]>();
    const seen = new Set<string>();
    for (const m of (models.data?.data as any[]) ?? []) {
      if (seen.has(m.id)) continue;
      seen.add(m.id);
      const owner = (m.owned_by as string) || "lainnya";
      if (!byProvider.has(owner)) byProvider.set(owner, []);
      byProvider.get(owner)!.push({ value: m.id, label: m.id });
    }
    const groups = [...byProvider.entries()].map(([provider, options]) => ({
      label: provider,
      options,
    }));
    const comboList = (combos.data?.data as any[]) ?? [];
    if (comboList.length) {
      groups.unshift({
        label: "⚡ Combo",
        options: comboList.map((c) => ({ value: c.name ?? c.id, label: `${c.name ?? c.id} (combo)` })),
      });
    }
    return groups;
  }, [models.data, combos.data]);

  const send = async () => {
    if (!prompt) return;
    setLoading(true);
    setOutput("");
    try {
      const res = await api.chat(
        { model, stream, messages: [{ role: "user", content: prompt }] },
        gwKey || undefined
      );
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        setOutput(`HTTP ${res.status}: ${body?.error?.message || res.statusText}`);
        return;
      }
      if (!stream) {
        const body = await res.json();
        setOutput(body.choices?.[0]?.message?.content || JSON.stringify(body, null, 2));
      } else {
        // SSE
        const reader = res.body!.getReader();
        const decoder = new TextDecoder();
        let acc = "";
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          acc += decoder.decode(value, { stream: true });
          const lines = acc.split("\n");
          acc = lines.pop() || "";
          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed.startsWith("data:")) continue;
            const payload = trimmed.slice(5).trim();
            if (payload === "[DONE]") continue;
            try {
              const chunk = JSON.parse(payload);
              const delta = chunk.choices?.[0]?.delta?.content;
              if (delta) setOutput((prev) => prev + delta);
            } catch {
              /* partial */
            }
          }
        }
      }
    } catch (e: any) {
      setOutput(`Error: ${e.message}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div>
      <h3>Playground</h3>
      {!gwKey && (
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 12 }}
          message="Belum ada gateway key — isi di bawah atau di halaman Login"
        />
      )}
      <Card>
        <Space direction="vertical" style={{ width: "100%" }} size={12}>
          <Space>
            <Input.Password
              placeholder="Gateway key (opsional)"
              value={gwKey}
              onChange={(e) => {
                setGwKey(e.target.value);
                setGatewayKey(e.target.value);
              }}
              style={{ width: 260 }}
            />
            <Select
              showSearch
              style={{ width: 320 }}
              value={model}
              onChange={setModel}
              options={grouped}
              filterOption={(input, opt) => String((opt as any)?.value ?? "").toLowerCase().includes(input.toLowerCase())}
            />
            <Switch checked={stream} onChange={setStream} checkedChildren="stream" unCheckedChildren="non-stream" />
          </Space>
          <Input.TextArea
            rows={4}
            placeholder="Prompt..."
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
          />
          <Button type="primary" loading={loading} onClick={send}>
            Kirim
          </Button>
          {output && (
            <pre style={{ background: "#111", padding: 12, borderRadius: 8, whiteSpace: "pre-wrap", minHeight: 100 }}>
              {output}
            </pre>
          )}
        </Space>
      </Card>
    </div>
  );
}
