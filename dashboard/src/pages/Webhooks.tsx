import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, Table, Button, Modal, Form, Input, Switch, Space, message, Popconfirm, Tabs, Tag, Typography } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { api } from "../api/client";

export default function Webhooks() {
  const qc = useQueryClient();
  const hooks = useQuery({ queryKey: ["webhooks"], queryFn: api.webhooks.list, refetchInterval: 5000 });
  const audit = useQuery({ queryKey: ["audit"], queryFn: api.audit, refetchInterval: 5000 });

  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const create = async (v: any) => {
    try {
      await api.webhooks.create(v);
      message.success("Webhook dibuat");
      setOpen(false);
      form.resetFields();
      qc.invalidateQueries({ queryKey: ["webhooks"] });
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const remove = async (id: string) => {
    await api.webhooks.remove(id);
    qc.invalidateQueries({ queryKey: ["webhooks"] });
  };

  return (
    <div>
      <Tabs
        items={[
          {
            key: "hooks",
            label: `Webhooks (${hooks.data?.data?.length || 0})`,
            children: (
              <Card
                title="Webhook Subscriptions"
                extra={
                  <Button type="primary" size="small" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
                    Tambah
                  </Button>
                }
              >
                <Table
                  size="small"
                  rowKey="id"
                  dataSource={hooks.data?.data || []}
                  loading={hooks.isLoading}
                  pagination={false}
                  columns={[
                    { title: "Nama", dataIndex: "name" },
                    { title: "URL", dataIndex: "url", render: (v) => <Typography.Text code>{v}</Typography.Text> },
                    {
                      title: "Events",
                      dataIndex: "events",
                      render: (v: string) => (
                        <Space wrap>
                          {v.split(",").map((e) => (
                            <Tag key={e} color={e.includes("error") ? "red" : "green"}>
                              {e.trim()}
                            </Tag>
                          ))}
                        </Space>
                      ),
                    },
                    { title: "Aktif", dataIndex: "is_active", render: (v) => (v ? "✅" : "⛔") },
                    {
                      title: "",
                      render: (_, r) => (
                        <Popconfirm title="Hapus?" onConfirm={() => remove(r.id)}>
                          <Button size="small" danger>
                            Hapus
                          </Button>
                        </Popconfirm>
                      ),
                    },
                  ]}
                />
              </Card>
            ),
          },
          {
            key: "audit",
            label: `Audit (${audit.data?.data?.length || 0})`,
            children: (
              <Card title="Audit Log">
                <Table
                  size="small"
                  rowKey="id"
                  dataSource={audit.data?.data || []}
                  loading={audit.isLoading}
                  pagination={{ pageSize: 25 }}
                  columns={[
                    { title: "Waktu", dataIndex: "ts", render: (v) => new Date(v.replace(" ", "T") + "Z").toLocaleString("id-ID") },
                    { title: "Action", dataIndex: "action", render: (v) => <Tag color={v === "delete" ? "red" : "blue"}>{v}</Tag> },
                    { title: "Resource", dataIndex: "resource" },
                    { title: "ID", dataIndex: "resource_id", render: (v) => (v ? <code>{v.slice(0, 12)}</code> : "—") },
                    { title: "Detail", dataIndex: "detail", render: (v) => v || "—" },
                  ]}
                />
              </Card>
            ),
          },
        ]}
      />

      <Modal title="Tambah Webhook" open={open} onCancel={() => setOpen(false)} onOk={() => form.submit()} destroyOnClose>
        <Form form={form} layout="vertical" onFinish={create} initialValues={{ events: "chat.success,chat.error", is_active: true }}>
          <Form.Item name="name" label="Nama" rules={[{ required: true }]}>
            <Input placeholder="notif-chat" />
          </Form.Item>
          <Form.Item name="url" label="Endpoint URL" rules={[{ required: true }]}>
            <Input placeholder="https://hooks.example.com/chat" />
          </Form.Item>
          <Form.Item name="events" label="Events (koma-pisah)">
            <Input placeholder="chat.success,chat.error,rate_limited" />
          </Form.Item>
          <Form.Item name="is_active" label="Aktif" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
