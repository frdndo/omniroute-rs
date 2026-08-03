import { useState } from "react";
import { Card, Typography, Tag, Space, Button, Select, message, Alert } from "antd";
import { PlayCircleOutlined } from "@ant-design/icons";
import { getGatewayKey } from "../api/client";

const SKILLS = [
  "listCapabilities",
  "providerDiscovery",
  "smartRouting",
  "quotaManagement",
  "costAnalysis",
  "healthReport",
];

export default function A2aPage() {
  const [skill, setSkill] = useState("healthReport");
  const [result, setResult] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const call = async () => {
    setBusy(true);
    setResult("");
    try {
      const key = getGatewayKey();
      const r = await fetch("/a2a", {
        method: "POST",
        headers: { "Content-Type": "application/json", ...(key ? { Authorization: `Bearer ${key}` } : {}) },
        body: JSON.stringify({ jsonrpc: "2.0", id: Date.now(), method: "skills/call", params: { skill } }),
      }).then((x) => x.json());
      setResult(r?.result?.result || JSON.stringify(r, null, 2).slice(0, 1500));
    } catch (e: any) {
      message.error(e.message);
      setResult(String(e.message));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <Typography.Title level={4}>A2A Protocol (Agent2Agent)</Typography.Title>
      <Alert
        type="info"
        showIcon
        message="Endpoint: POST /a2a · Agent Card: GET /.well-known/agent-card.json"
        description="Protokol Google agent↔agent — agent lain bisa panggil skill router kita (providerDiscovery, smartRouting, quotaManagement, costAnalysis, healthReport, listCapabilities)"
        style={{ marginBottom: 16 }}
      />

      <Card title="Agent Card">
        <pre style={{ background: "#111", color: "#7ee787", padding: 12, borderRadius: 6, overflow: "auto", maxHeight: 220 }}>
          {JSON.stringify({ name: "omniroute-rs", protocolVersion: "0.3", skills: SKILLS }, null, 2)}
        </pre>
      </Card>

      <Card title="Test Skill" style={{ marginTop: 16 }}>
        <Space direction="vertical" style={{ width: "100%" }}>
          <Space>
            <Select value={skill} onChange={setSkill} style={{ width: 220 }} options={SKILLS.map((s) => ({ value: s, label: s }))} />
            <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={call}>
              Call Skill
            </Button>
          </Space>
          {result && (
            <pre style={{ background: "#111", color: "#7ee787", padding: 12, borderRadius: 6, maxHeight: 300, overflow: "auto", whiteSpace: "pre-wrap" }}>
              {result}
            </pre>
          )}
        </Space>
      </Card>

      <Card title="Integrasi" style={{ marginTop: 16 }}>
        <Typography.Paragraph>
          <Tag>POST /a2a</Tag> dengan JSON-RPC: <code>agent/getCard</code>, <code>skills/call</code> ({"{"}skill{"}"}),{" "}
          <code>message/send</code> ({"{"}message.text, model{"}"}), <code>message/get</code> ({"{"}taskId{"}"}).
        </Typography.Paragraph>
        <Typography.Paragraph type="secondary">
          message/send mengembalikan task (completed/failed) dengan artifacts dari routing engine — termasuk fallback,
          scoring, affinity, cache.
        </Typography.Paragraph>
      </Card>
    </div>
  );
}
