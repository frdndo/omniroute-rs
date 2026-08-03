import { useState } from "react";
import { Card, Typography, Tag, Space, Input, Button, Select, message, Alert } from "antd";
import { PlayCircleOutlined } from "@ant-design/icons";
import { api, getGatewayKey } from "../api/client";

const TOOLS = [
  { name: "chat", desc: "Route chat melalui smart router (fallback, scoring, affinity)", args: ["model", "messages", "session_id?"] },
  { name: "list_models", desc: "Semua model yang bisa dilayani router", args: [] },
  { name: "server_status", desc: "Health: version, uptime, provider aktif, total request", args: [] },
];

export default function McpPage() {
  const [tool, setTool] = useState("chat");
  const [model, setModel] = useState("gpt-4o");
  const [prompt, setPrompt] = useState("Halo, perkenalkan dirimu");
  const [result, setResult] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const call = async () => {
    setBusy(true);
    setResult("");
    try {
      const params: any = tool === "chat" ? { name: "chat", arguments: { model, messages: [{ role: "user", content: prompt }] } } : { name: tool, arguments: {} };
      const r = await api.mcpCall(params, getGatewayKey());
      const content = r?.result?.content?.[0]?.text;
      setResult(content || JSON.stringify(r, null, 2).slice(0, 2000));
    } catch (e: any) {
      message.error(e.message);
      setResult(String(e.message));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <Typography.Title level={4}>MCP Server</Typography.Title>
      <Alert
        type="info"
        showIcon
        message="Endpoint: POST /mcp (JSON-RPC 2.0, Streamable HTTP)"
        description="Pakai di Claude Desktop / Cursor: MCP client connect ke http://localhost:20129/mcp dengan Authorization: Bearer &lt;gateway-key&gt;"
        style={{ marginBottom: 16 }}
      />

      <Card title="Tools Tersedia">
        {TOOLS.map((t) => (
          <div key={t.name} style={{ marginBottom: 12, padding: 8, background: "#fafafa", borderRadius: 6 }}>
            <Space>
              <Tag color="blue">{t.name}</Tag>
              <Typography.Text>{t.desc}</Typography.Text>
            </Space>
            <div style={{ fontSize: 12, color: "#888", marginTop: 4 }}>args: {t.args.join(", ") || "—"}</div>
          </div>
        ))}
      </Card>

      <Card title="Test Call" style={{ marginTop: 16 }}>
        <Space direction="vertical" style={{ width: "100%" }}>
          <Space>
            <Select value={tool} onChange={setTool} style={{ width: 180 }} options={TOOLS.map((t) => ({ value: t.name, label: t.name }))} />
            {tool === "chat" && (
              <>
                <Input value={model} onChange={(e) => setModel(e.target.value)} style={{ width: 200 }} placeholder="model" />
              </>
            )}
          </Space>
          {tool === "chat" && <Input.TextArea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3} />}
          <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={call}>
            Call Tool
          </Button>
          {result && (
            <pre style={{ background: "#111", color: "#7ee787", padding: 12, borderRadius: 6, maxHeight: 300, overflow: "auto", whiteSpace: "pre-wrap" }}>
              {result}
            </pre>
          )}
        </Space>
      </Card>
    </div>
  );
}
