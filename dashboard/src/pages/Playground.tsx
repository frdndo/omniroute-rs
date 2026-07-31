import { useState } from "react";
import { Card, Input, Button, Select, Space, Switch, Alert } from "antd";
import { useQuery } from "@tanstack/react-query";
import { api, getGatewayKey, setGatewayKey } from "../api/client";

export default function Playground() {
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const [model, setModel] = useState<string>("gpt-4o");
  const [prompt, setPrompt] = useState("");
  const [stream, setStream] = useState(false);
  const [loading, setLoading] = useState(false);
  const [output, setOutput] = useState("");
  const [gwKey, setGwKey] = useState(getGatewayKey());

  const modelOptions = (models.data?.data as any[])?.map((m: any) => ({ value: m.id, label: m.id })) || [];

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
              options={modelOptions}
              filterOption={(input, opt) => (opt?.value as string)?.toLowerCase().includes(input.toLowerCase())}
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
