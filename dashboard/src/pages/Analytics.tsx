import { useQuery } from "@tanstack/react-query";
import { Card, Row, Col, Statistic, Table, Typography, Spin, Alert, Empty } from "antd";
import ReactECharts from "echarts-for-react";
import { api } from "../api/client";

export default function Analytics() {
  const q = useQuery({
    queryKey: ["stats"],
    queryFn: api.stats,
    refetchInterval: 5000,
  });

  if (q.isError) return <Alert type="error" message="Gagal ambil stats" description={String(q.error?.message || q.error)} showIcon />;
  if (q.isLoading) return <Spin />;

  const s = q.data;
  if (!s || s.error) return <Alert type="warning" message={s?.error || "no data"} showIcon />;

  const hourly = s.hourly || [];
  const byProvider = s.by_provider || [];
  const byStatus = s.by_status || {};

  const hourlyOption = {
    backgroundColor: "transparent",
    tooltip: { trigger: "axis" },
    grid: { left: 40, right: 16, top: 24, bottom: 32 },
    xAxis: { type: "category", data: hourly.map((h: any) => h.bucket.slice(11)), axisLabel: { color: "#aaa" } },
    yAxis: { type: "value", axisLabel: { color: "#aaa" } },
    series: [{ name: "Requests", type: "line", smooth: true, areaStyle: {}, data: hourly.map((h: any) => h.count) }],
  };

  const statusOption = {
    tooltip: { trigger: "item" },
    legend: { bottom: 0, textStyle: { color: "#aaa" } },
    series: [
      {
        type: "pie",
        radius: ["40%", "65%"],
        data: [
          { value: byStatus["2xx"] || 0, name: "2xx", itemStyle: { color: "#52c41a" } },
          { value: byStatus["3xx"] || 0, name: "3xx", itemStyle: { color: "#1677ff" } },
          { value: byStatus["4xx"] || 0, name: "4xx", itemStyle: { color: "#faad14" } },
          { value: byStatus["5xx"] || 0, name: "5xx", itemStyle: { color: "#ff4d4f" } },
        ],
      },
    ],
  };

  const providerOption = {
    tooltip: { trigger: "axis" },
    grid: { left: 90, right: 24, top: 16, bottom: 32 },
    xAxis: { type: "value", axisLabel: { color: "#aaa" } },
    yAxis: { type: "category", data: byProvider.map((p: any) => p.provider), axisLabel: { color: "#aaa" } },
    series: [{ type: "bar", data: byProvider.map((p: any) => p.requests), itemStyle: { color: "#1677ff" } }],
  };

  return (
    <div>
      <Typography.Title level={4}>Analytics</Typography.Title>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Total Requests" value={s.total_requests || 0} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Error (4xx+5xx)" value={s.total_errors || 0} valueStyle={{ color: (s.total_errors || 0) > 0 ? "#ff4d4f" : undefined }} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Avg Latency" value={Math.round((s.avg_duration_ms || 0) * 10) / 10} suffix="ms" />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Provider Dipakai" value={byProvider.length} />
          </Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} lg={14}>
          <Card title="Request per Jam (24h)">
            {hourly.length ? <ReactECharts option={hourlyOption} style={{ height: 260 }} /> : <Empty description="belum ada data" />}
          </Card>
        </Col>
        <Col xs={24} lg={10}>
          <Card title="Status">
            {s.total_requests ? <ReactECharts option={statusOption} style={{ height: 260 }} /> : <Empty description="belum ada data" />}
          </Card>
        </Col>
      </Row>

      <Card title="Per Provider" style={{ marginTop: 16 }}>
        {byProvider.length ? (
          <>
            <ReactECharts option={providerOption} style={{ height: 220 }} />
            <Table
              size="small"
              rowKey="provider"
              dataSource={byProvider}
              pagination={false}
              style={{ marginTop: 8 }}
              columns={[
                { title: "Provider", dataIndex: "provider" },
                { title: "Requests", dataIndex: "requests" },
                { title: "Avg Latency (ms)", dataIndex: "avg_duration_ms", render: (v) => Math.round(v * 10) / 10 },
              ]}
            />
          </>
        ) : (
          <Empty description="belum ada data chat" />
        )}
      </Card>
    </div>
  );
}
