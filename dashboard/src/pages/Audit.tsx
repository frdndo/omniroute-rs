import { useQuery } from "@tanstack/react-query";
import { Table, Tag } from "antd";
import { api } from "../api/client";

export default function Audit() {
  const q = useQuery({ queryKey: ["audit"], queryFn: api.audit });

  return (
    <div>
      <h3 style={{ marginBottom: 12 }}>Audit Log</h3>
      <Table
        rowKey="id"
        dataSource={q.data?.data || []}
        loading={q.isLoading}
        pagination={{ pageSize: 25 }}
        size="small"
        columns={[
          { title: "Waktu", dataIndex: "ts", width: 170, render: (v) => <span style={{ fontFamily: "monospace", fontSize: 12 }}>{v}</span> },
          { title: "Aksi", dataIndex: "action", width: 100, render: (v) => <Tag color="geekblue">{v}</Tag> },
          { title: "Resource", dataIndex: "resource", width: 120 },
          { title: "ID", dataIndex: "resource_id", width: 120, render: (v) => (v ? <span style={{ fontFamily: "monospace", fontSize: 12 }}>{v}</span> : "—") },
          { title: "Detail", dataIndex: "detail", render: (v) => (v ? <span style={{ fontSize: 12 }}>{v}</span> : "—") },
        ]}
      />
    </div>
  );
}
