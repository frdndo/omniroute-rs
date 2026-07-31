import { useQuery } from "@tanstack/react-query";
import { Card, Row, Col, Statistic, Table, Typography, Spin, Alert } from "antd";
import { api } from "../api/client";

export default function Status() {
  const health = useQuery({ queryKey: ["health"], queryFn: api.health });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const providers = useQuery({ queryKey: ["providers"], queryFn: api.providers.list });
  const keys = useQuery({ queryKey: ["keys"], queryFn: api.keys.list });
  const combos = useQuery({ queryKey: ["combos"], queryFn: api.combos.list });
  const logs = useQuery({ queryKey: ["logs"], queryFn: api.logs });

  if (health.isError || providers.isError) {
    return <Alert type="error" message="Gagal terhubung ke proxy" description={String((health.error || providers.error)?.message)} showIcon />;
  }

  const loading = health.isLoading || providers.isLoading;
  const activeProviders = (providers.data?.data || []).filter((p) => p.is_active).length;
  const activeKeys = (keys.data?.data || []).filter((k) => k.is_active).length;
  const modelCount = (models.data?.data as any[])?.length ?? 0;
  const uptime = logs.data?.uptime_seconds ?? 0;

  const statData = [
    { title: "Status", value: health.data?.status === "ok" ? "Healthy" : "? ", suffix: health.data?.version || "" },
    { title: "Providers Aktif", value: activeProviders, suffix: `/ ${providers.data?.data.length || 0} total` },
    { title: "API Keys Aktif", value: activeKeys, suffix: `/ ${keys.data?.data.length || 0} total` },
    { title: "Combos", value: combos.data?.data.length || 0 },
    { title: "Model Tersedia", value: modelCount },
    { title: "Uptime", value: Math.floor(uptime / 3600), suffix: "jam" },
  ];

  return (
    <div>
      <Typography.Title level={4}>Status</Typography.Title>
      {loading ? (
        <Spin />
      ) : (
        <>
          <Row gutter={[16, 16]}>
            {statData.map((s) => (
              <Col xs={12} md={8} key={s.title}>
                <Card>
                  <Statistic title={s.title} value={s.value} suffix={s.suffix} />
                </Card>
              </Col>
            ))}
          </Row>
          <Card title="Provider Connections" style={{ marginTop: 16 }}>
            <Table
              size="small"
              rowKey="id"
              dataSource={providers.data?.data || []}
              pagination={false}
              columns={[
                { title: "Provider", dataIndex: "provider" },
                { title: "Nama", dataIndex: "name" },
                { title: "API Key", dataIndex: "api_key", render: (v) => <code>{v}</code> },
                { title: "Aktif", dataIndex: "is_active", render: (v) => (v ? "✅" : "⛔") },
                { title: "Priority", dataIndex: "priority" },
              ]}
            />
          </Card>
        </>
      )}
    </div>
  );
}
