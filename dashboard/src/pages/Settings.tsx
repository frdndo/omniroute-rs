import { useQuery } from "@tanstack/react-query";
import { Card, Row, Col, Statistic, Table, Tag, Typography, Spin, Alert } from "antd";
import { api } from "../api/client";

export default function Settings() {
  const q = useQuery({ queryKey: ["settings"], queryFn: api.settings, refetchInterval: 10000 });

  if (q.isError) return <Alert type="error" message="Gagal ambil settings" showIcon />;
  if (q.isLoading) return <Spin />;

  const s = q.data;

  return (
    <div>
      <Typography.Title level={4}>Settings & Status</Typography.Title>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={4}>
          <Card>
            <Statistic title="Version" value={s.version} />
          </Card>
        </Col>
        <Col xs={12} md={4}>
          <Card>
            <Statistic title="Uptime" value={s.uptime_seconds} suffix="s" />
          </Card>
        </Col>
        <Col xs={12} md={4}>
          <Card>
            <Statistic title="Providers (registry)" value={s.providers_registry} />
          </Card>
        </Col>
        <Col xs={12} md={4}>
          <Card>
            <Statistic title="Models (registry)" value={s.models_registry} />
          </Card>
        </Col>
        <Col xs={12} md={4}>
          <Card>
            <Statistic title="DB" value={s.db_connected ? "SQLite ✓" : "off"} valueStyle={{ color: s.db_connected ? "#52c41a" : "#ff4d4f" }} />
          </Card>
        </Col>
      </Row>

      <Card title="Feature Flags" style={{ marginTop: 16 }}>
        {(s.features || []).map((f: string) => (
          <Tag key={f} color="blue" style={{ marginBottom: 6 }}>
            {f}
          </Tag>
        ))}
      </Card>

      <Card title="Environment" style={{ marginTop: 16 }}>
        <Table
          size="small"
          rowKey={(r: any) => r[0]}
          pagination={false}
          dataSource={Object.entries(s.env || {}).map(([k, v]) => [k, v])}
          columns={[
            { title: "Variable", dataIndex: 0, render: (v) => <code>{v}</code> },
            { title: "Nilai", dataIndex: 1 },
          ]}
        />
      </Card>

      <Card title="Database" style={{ marginTop: 16 }}>
        <Typography.Paragraph>
          Path: <code>{s.db_path}</code> · Started: <code>{new Date(s.started_at).toLocaleString("id-ID")}</code>
        </Typography.Paragraph>
      </Card>
    </div>
  );
}
