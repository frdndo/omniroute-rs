import { useState } from "react";
import { Card, Typography, Input, Button, Space, Table, Tag, message, Alert } from "antd";
import { PlayCircleOutlined } from "@ant-design/icons";
import { getGatewayKey } from "../api/client";

export default function BatchPage() {
  const [prompts, setPrompts] = useState("apa itu rust\nexplain tauri\nhello world");
  const [model, setModel] = useState("gpt-4o");
  const [batchId, setBatchId] = useState("");
  const [result, setResult] = useState<any>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      const key = getGatewayKey();
      const requests = prompts.split("\n").filter(Boolean).map((p, i) => ({
        custom_id: `req-${i}`,
        url: "/v1/chat/completions",
        body: { model, messages: [{ role: "user", content: p }] },
      }));
      const r = await fetch("/v1/batch", {
        method: "POST",
        headers: { "Content-Type": "application/json", ...(key ? { Authorization: `Bearer ${key}` } : {}) },
        body: JSON.stringify({ requests }),
      }).then((x) => x.json());
      setBatchId(r.id);
      setResult(r);
      message.success(`Batch ${r.status}: ${r.request_counts?.succeeded}/${r.request_counts?.total} sukses`);
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setBusy(false);
    }
  };

  const getBatch = async () => {
    if (!batchId) return;
    const key = getGatewayKey();
    const r = await fetch(`/v1/batch/${batchId}`, {
      headers: key ? { Authorization: `Bearer ${key}` } : {},
    }).then((x) => x.json());
    setResult(r);
  };

  const results = result?.results || [];

  return (
    <div>
      <Typography.Title level={4}>Batch + Relay</Typography.Title>
      <Alert
        type="info"
        showIcon
        message="Batch API (OpenAI-style) + generic relay"
        description="POST /v1/batch · GET /v1/batch/{id} · POST /v1/batch/{id}/cancel · POST /v1/relay"
        style={{ marginBottom: 16 }}
      />

      <Card title="Submit Batch">
        <Space direction="vertical" style={{ width: "100%" }}>
          <Space>
            <span>Model:</span>
            <Input value={model} onChange={(e) => setModel(e.target.value)} style={{ width: 200 }} />
          </Space>
          <Input.TextArea
            value={prompts}
            onChange={(e) => setPrompts(e.target.value)}
            rows={4}
            placeholder="satu prompt per baris"
          />
          <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={submit}>
            Submit Batch
          </Button>
          {batchId && (
            <Space>
              <code>{batchId}</code>
              <Button size="small" onClick={getBatch}>
                Get Status
              </Button>
            </Space>
          )}
        </Space>
      </Card>

      {results.length > 0 && (
        <Card title="Hasil" style={{ marginTop: 16 }}>
          <Table
            size="small"
            rowKey="custom_id"
            dataSource={results}
            pagination={false}
            columns={[
              { title: "custom_id", dataIndex: "custom_id" },
              { title: "Status", dataIndex: "status", render: (v) => <Tag color={v === "succeeded" ? "green" : "red"}>{v}</Tag> },
              { title: "Provider", dataIndex: "provider" },
              {
                title: "Output",
                dataIndex: "output",
                render: (v) => v?.choices?.[0]?.message?.content?.slice(0, 80) || v?.error || "—",
              },
            ]}
          />
        </Card>
      )}

      <Card title="Relay (dokumentasi)" style={{ marginTop: 16 }}>
        <Typography.Paragraph>
          <code>POST /v1/relay</code> dengan body: <code>{"{ url, method, headers?, body?, timeout? }"}</code> — forwarder
          generik ke endpoint http(s) apa pun. SSRF-guard: hanya http/https yang diterima.
        </Typography.Paragraph>
      </Card>
    </div>
  );
}
