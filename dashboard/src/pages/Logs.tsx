import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Table, Tag, Typography, Select, Space, Input } from "antd";
import { api } from "../api/client";
import type { LogEntry } from "../api/client";

const STATUS_COLOR: Record<string, string> = {
  "2": "green",
  "3": "blue",
  "4": "orange",
  "5": "red",
};

export default function Logs() {
  const [filter, setFilter] = useState<string>("all");
  const [search, setSearch] = useState("");

  const q = useQuery({
    queryKey: ["logs", filter, search],
    queryFn: api.logs,
    refetchInterval: 2000, // poll every 2s
  });

  const rows: LogEntry[] = (q.data?.data || []).filter((e) => {
    if (filter !== "all" && String(e.status).startsWith(filter)) return false;
    if (search && !e.uri.toLowerCase().includes(search.toLowerCase())) return false;
    return true;
  });

  return (
    <div>
      <Space style={{ marginBottom: 12, justifyContent: "space-between", width: "100%" }}>
        <h3 style={{ margin: 0 }}>Log Request (realtime)</h3>
        <Space>
          <Select
            value={filter}
            onChange={setFilter}
            style={{ width: 140 }}
            options={[
              { value: "all", label: "Semua" },
              { value: "2", label: "2xx sukses" },
              { value: "4", label: "4xx error" },
              { value: "5", label: "5xx error" },
            ]}
          />
          <Input.Search placeholder="cari path..." allowClear onSearch={setSearch} style={{ width: 220 }} />
        </Space>
      </Space>
      <Table
        rowKey={(e) => `${e.ts}-${e.uri}-${Math.random()}`}
        size="small"
        dataSource={rows}
        loading={q.isLoading}
        pagination={{ pageSize: 25 }}
        columns={[
          { title: "Waktu", dataIndex: "ts", render: (v) => new Date(v).toLocaleTimeString("id-ID"), width: 110 },
          { title: "Method", dataIndex: "method", width: 80 },
          { title: "Path", dataIndex: "uri", render: (v) => <Typography.Text code>{v}</Typography.Text> },
          {
            title: "Status",
            dataIndex: "status",
            width: 90,
            render: (v) => <Tag color={STATUS_COLOR[String(v)[0]] || "default"}>{v}</Tag>,
          },
          { title: "Durasi", dataIndex: "duration_ms", width: 90, render: (v) => `${v} ms` },
        ]}
      />
    </div>
  );
}
