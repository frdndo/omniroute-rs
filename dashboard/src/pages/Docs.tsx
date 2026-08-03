import { Card, Typography, Table, Tag } from "antd";

const ENDPOINTS = [
  ["POST", "/v1/chat/completions", "Chat routing penuh (fallback, scoring, affinity, cache, compress)"],
  ["GET", "/v1/models", "List 3,012 model dari registry"],
  ["POST", "/v1/batch", "Batch API gaya OpenAI (submit + eksekusi)"],
  ["GET", "/v1/batch/{id}", "Status + hasil batch"],
  ["POST", "/v1/batch/{id}/cancel", "Batalkan batch"],
  ["POST", "/v1/relay", "Forwarder HTTP generik (SSRF-guard)"],
  ["POST", "/mcp", "MCP server (JSON-RPC): chat, list_models, server_status"],
  ["POST", "/a2a", "A2A protocol: agent/getCard, skills/call, message/send"],
  ["GET", "/.well-known/agent-card.json", "A2A agent card"],
  ["GET", "/health", "Health check"],
  ["GET", "/admin/providers", "CRUD provider connections (key masked)"],
  ["GET", "/admin/api-keys", "CRUD gateway API keys"],
  ["GET", "/admin/combos", "CRUD combos"],
  ["GET", "/admin/stats", "Telemetry agregat (analytics)"],
  ["GET", "/admin/costs", "Spend + budget bulanan"],
  ["GET", "/admin/pricing", "Pricing editor ($/MTok)"],
  ["GET", "/admin/budgets", "Budget bulanan per provider"],
  ["GET", "/admin/webhooks", "Webhook subscriptions"],
  ["GET", "/admin/audit", "Audit log"],
  ["GET", "/admin/cache", "Cache stats + entries"],
  ["GET", "/admin/logs", "Ring buffer request log"],
  ["GET", "/admin/settings", "Konfigurasi runtime"],
];

export default function Docs() {
  return (
    <div>
      <Typography.Title level={4}>API Reference</Typography.Title>
      <Card>
        <Typography.Paragraph>
          Semua endpoint non-admin butuh <Tag>Authorization: Bearer &lt;gateway-key&gt;</Tag>. Semua endpoint{" "}
          <Tag color="red">/admin/*</Tag> butuh <Tag>Authorization: Bearer &lt;admin-key&gt;</Tag> (fail-closed 503 tanpa
          admin keys).
        </Typography.Paragraph>
        <Table
          size="small"
          rowKey={(r) => r[0] + r[1]}
          pagination={{ pageSize: 25 }}
          dataSource={ENDPOINTS}
          columns={[
            { title: "Method", dataIndex: 0, width: 80, render: (v) => <Tag color={v === "GET" ? "green" : "blue"}>{v}</Tag> },
            { title: "Path", dataIndex: 1, render: (v) => <code>{v}</code> },
            { title: "Deskripsi", dataIndex: 2 },
          ]}
        />
      </Card>

      <Card title="Env Vars" style={{ marginTop: 16 }}>
        <Table
          size="small"
          rowKey={(r) => r[0]}
          pagination={false}
          dataSource={[
            ["OMNIROUTE_PORT", "Port server (test: 20129)"],
            ["OMNIROUTE_DB_PATH", "Path SQLite (default ./data/omniroute.db)"],
            ["OMNIROUTE_PROVIDER_KEYS", "Env fallback provider keys (format: provider=sk-key,...)"],
            ["OMNIROUTE_BASE_URL_<PROVIDER>", "Override base URL (suffix /v1)"],
            ["OMNIROUTE_API_KEYS", "Gateway API keys (comma-separated)"],
            ["OMNIROUTE_ADMIN_KEYS", "Admin keys — wajib, fail-closed"],
            ["OMNIROUTE_ALLOWED_HOSTS", "Host guard (403 untuk spoof)"],
            ["OMNIROUTE_VERSION", "Override version string"],
            ["RUST_LOG", "Log level (info/debug)"],
          ]}
          columns={[
            { title: "Variable", dataIndex: 0, render: (v) => <code>{v}</code> },
            { title: "Keterangan", dataIndex: 1 },
          ]}
        />
      </Card>
    </div>
  );
}
