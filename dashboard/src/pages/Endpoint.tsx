import { Card, Typography, Table, Tag } from "antd";
import { apiBase } from "../api/client";

const { Title, Text, Paragraph } = Typography;

const BASE_URL = apiBase || "http://localhost:20129";

const ENDPOINTS = [
  {
    method: "POST",
    path: "/v1/chat/completions",
    desc: "Chat completion (OpenAI-compatible) — routing via combo engine",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/v1/chat/completions \\
  -H "Authorization: Bearer sk-xxxx" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"deepseek-v4-flash-free","messages":[{"role":"user","content":"halo"}]}'`,
  },
  {
    method: "GET",
    path: "/v1/models",
    desc: "Daftar semua model (1.940 curated + synced)",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/v1/models -H "Authorization: Bearer sk-xxxx"`,
  },
  {
    method: "POST",
    path: "/mcp",
    desc: "MCP Server (JSON-RPC) — tools/call, tools/list, dll",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/mcp \\
  -H "Authorization: Bearer sk-xxxx" -H "Content-Type: application/json" \\
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'`,
  },
  {
    method: "POST",
    path: "/a2a",
    desc: "A2A Protocol v0.3 — message/send, skills/list",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/a2a \\
  -H "Authorization: Bearer sk-xxxx" -H "Content-Type: application/json" \\
  -d '{"jsonrpc":"2.0","id":1,"method":"skills/list"}'`,
  },
  {
    method: "POST",
    path: "/v1/batch",
    desc: "Batch submit — banyak request sekali kirim",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/v1/batch \\
  -H "Authorization: Bearer sk-xxxx" -H "Content-Type: application/json" \\
  -d '{"inputs":[{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}]}'`,
  },
  {
    method: "POST",
    path: "/v1/relay",
    desc: "Relay — teruskan request ke base URL lain (http(s))",
    auth: "Gateway key",
    curl: `curl ${BASE_URL}/v1/relay \\
  -H "Authorization: Bearer sk-xxxx" -H "Content-Type: application/json" \\
  -d '{"url":"https://api.example.com/v1/chat/completions","body":{}}'`,
  },
  {
    method: "GET",
    path: "/health",
    desc: "Liveness probe — publik tanpa auth",
    auth: "—",
    curl: `curl ${BASE_URL}/health`,
  },
  {
    method: "GET",
    path: "/admin/settings",
    desc: "Admin API (providers, keys, combos, quota, audit...)",
    auth: "Admin key",
    curl: `curl ${BASE_URL}/admin/settings -H "Authorization: Bearer sk-admin"`,
  },
];

export default function Endpoint() {
  return (
    <div>
      <Title level={3} style={{ marginTop: 0 }}>
        Endpoint
      </Title>
      <Card style={{ marginBottom: 16 }}>
        <Text strong>Base URL:</Text>{" "}
        <Text code copyable>
          {BASE_URL}
        </Text>
        <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0 }}>
          Semua API OpenAI-compatible bisa langsung pakai proxy ini — ganti
          base URL client (OpenAI SDK, LangChain, Cursor, dll) ke{" "}
          {BASE_URL} dan pakai gateway key sebagai API key.
        </Paragraph>
      </Card>
      <Table
        rowKey="path"
        dataSource={ENDPOINTS}
        pagination={false}
        size="small"
        expandable={{
          expandedRowRender: (r) => (
            <pre
              style={{
                background: "#1f1f1f",
                padding: 12,
                borderRadius: 6,
                overflowX: "auto",
                fontSize: 12,
              }}
            >
              {r.curl}
            </pre>
          ),
        }}
        columns={[
          {
            title: "Method",
            dataIndex: "method",
            width: 80,
            render: (m) => (
              <Tag color={m === "GET" ? "green" : m === "POST" ? "blue" : "orange"}>{m}</Tag>
            ),
          },
          { title: "Path", dataIndex: "path", render: (p) => <Text code>{p}</Text> },
          { title: "Deskripsi", dataIndex: "desc" },
          { title: "Auth", dataIndex: "auth", width: 110, render: (a) => <Tag>{a}</Tag> },
        ]}
      />
    </div>
  );
}
