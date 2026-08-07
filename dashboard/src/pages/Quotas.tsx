import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, InputNumber, Select, Space, Tag, Progress, message, Popconfirm } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { api } from "../api/client";

export default function Quotas() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["quotas"], queryFn: api.quotas.list });
  const keys = useQuery({ queryKey: ["api-keys"], queryFn: api.keys.list });
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const invalidate = () => qc.invalidateQueries({ queryKey: ["quotas"] });

  const create = async (v: any) => {
    try {
      await api.quotas.create(v);
      message.success("Quota dibuat");
      setOpen(false);
      form.resetFields();
      invalidate();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const remove = async (id: string) => {
    await api.quotas.remove(id);
    invalidate();
  };

  const keyOptions = (keys.data?.data as any[])?.map((k: any) => ({
    value: k.id,
    label: `${k.name || k.id} (${String(k.key).slice(0, 8)}…)`,
  })) || [];

  return (
    <div>
      <Space style={{ marginBottom: 12, justifyContent: "space-between", width: "100%" }}>
        <h3 style={{ margin: 0 }}>Quota per API Key</h3>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
          Buat Quota
        </Button>
      </Space>
      <Table
        rowKey="id"
        dataSource={q.data?.data || []}
        loading={q.isLoading}
        pagination={{ pageSize: 10 }}
        columns={[
          { title: "API Key", dataIndex: "key_name" },
          { title: "Unit", dataIndex: "unit", render: (v) => <Tag>{v}</Tag> },
          { title: "Limit", dataIndex: "limit", render: (v, r) => (r.unit === "usd" ? `$${v}` : v.toLocaleString()) },
          { title: "Window", dataIndex: "window", render: (v) => <Tag color="blue">{v}</Tag> },
          { title: "Policy", dataIndex: "policy", render: (v) => <Tag color={v === "hard" ? "red" : "orange"}>{v}</Tag> },
          {
            title: "Pemakaian",
            render: (_, r) => (
              <div style={{ width: 160 }}>
                <Progress
                  percent={Math.min(100, Math.round(r.used_pct || 0))}
                  size="small"
                  status={r.used_pct >= 100 ? "exception" : r.used_pct >= 80 ? "active" : "normal"}
                  format={() => `${Math.round(r.used || 0).toLocaleString()} / ${r.limit.toLocaleString()}`}
                />
              </div>
            ),
          },
          {
            title: "Aksi",
            render: (_, r) => (
              <Popconfirm title="Hapus quota?" onConfirm={() => remove(r.id)}>
                <Button size="small" danger>
                  Hapus
                </Button>
              </Popconfirm>
            ),
          },
        ]}
      />
      <Modal title="Buat Quota" open={open} onCancel={() => setOpen(false)} onOk={() => form.submit()} destroyOnClose>
        <Form form={form} layout="vertical" onFinish={create} initialValues={{ unit: "tokens", window: "daily", policy: "hard" }}>
          <Form.Item name="api_key_id" label="API Key" rules={[{ required: true }]}>
            <Select options={keyOptions} showSearch optionFilterProp="label" placeholder="Pilih API key" />
          </Form.Item>
          <Space size={16} style={{ display: "flex" }}>
            <Form.Item name="unit" label="Unit" rules={[{ required: true }]}>
              <Select
                style={{ width: 140 }}
                options={[
                  { value: "requests", label: "requests" },
                  { value: "tokens", label: "tokens" },
                  { value: "usd", label: "usd ($)" },
                ]}
              />
            </Form.Item>
            <Form.Item name="limit" label="Limit" rules={[{ required: true }]}>
              <InputNumber min={1} style={{ width: 160 }} placeholder="100000" />
            </Form.Item>
          </Space>
          <Space size={16} style={{ display: "flex" }}>
            <Form.Item name="window" label="Window" rules={[{ required: true }]}>
              <Select
                style={{ width: 140 }}
                options={[
                  { value: "hourly", label: "hourly" },
                  { value: "daily", label: "daily" },
                  { value: "weekly", label: "weekly" },
                  { value: "monthly", label: "monthly" },
                ]}
              />
            </Form.Item>
            <Form.Item name="policy" label="Policy" rules={[{ required: true }]}>
              <Select
                style={{ width: 140 }}
                options={[
                  { value: "hard", label: "hard (blokir)" },
                  { value: "soft", label: "soft (warn)" },
                ]}
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </div>
  );
}
