import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, Row, Col, Statistic, Table, Button, Popconfirm, message, Tag, Space } from "antd";
import { api } from "../api/client";

export default function CachePage() {
  const qc = useQueryClient();
  const cache = useQuery({ queryKey: ["cache"], queryFn: api.cache.list, refetchInterval: 5000 });

  const flush = async () => {
    await api.cache.clear();
    message.success("Cache dibersihkan");
    qc.invalidateQueries({ queryKey: ["cache"] });
  };

  const remove = async (key: string) => {
    await api.cache.remove(key);
    qc.invalidateQueries({ queryKey: ["cache"] });
  };

  const c = cache.data;
  const entries = c?.entries_list || [];

  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Cache Entries" value={c?.entries ?? 0} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Total Hits" value={c?.total_hits ?? 0} />
          </Card>
        </Col>
        <Col xs={24} md={12}>
          <Card title="Aksi">
            <Space>
              <Popconfirm title="Flush SEMUA cache?" onConfirm={flush}>
                <Button danger>Flush Semua</Button>
              </Popconfirm>
              <Button onClick={() => qc.invalidateQueries({ queryKey: ["cache"] })}>Refresh</Button>
            </Space>
            <div style={{ marginTop: 8, fontSize: 12, color: "#888" }}>
              Aktifkan cache per-request dengan body <code>{"{ \"cache\": true }"}</code> — default TTL 300s
              (atau <code>cache_ttl</code> detik).
            </div>
          </Card>
        </Col>
      </Row>

      <Card title="Cache Entries" style={{ marginTop: 16 }}>
        <Table
          size="small"
          rowKey="key"
          dataSource={entries}
          loading={cache.isLoading}
          pagination={{ pageSize: 20 }}
          columns={[
            {
              title: "Key",
              dataIndex: "key",
              render: (v) => (
                <Space>
                  <code>{v.slice(0, 16)}…</code>
                  {v.length > 0 && <Tag color={v.startsWith("cache") ? "blue" : "geekblue"}>sha256</Tag>}
                </Space>
              ),
            },
            { title: "Model", dataIndex: "model", render: (v) => <Tag>{v}</Tag> },
            { title: "Hits", dataIndex: "hits" },
            { title: "Dibuat", dataIndex: "created_at" },
            { title: "Expires", dataIndex: "expires_at" },
            {
              title: "",
              render: (_, r: any) => (
                <Popconfirm title="Hapus entry ini?" onConfirm={() => remove(r.key)}>
                  <Button size="small" danger>
                    Hapus
                  </Button>
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>
    </div>
  );
}
