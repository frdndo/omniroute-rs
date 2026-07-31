import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, Input, InputNumber, Switch, Space, Tag, message, Popconfirm } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { api } from "../api/client";
import type { ProviderConnection } from "../api/client";

export default function Providers() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["providers"], queryFn: api.providers.list });
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<ProviderConnection | null>(null);
  const [form] = Form.useForm();

  const invalidate = () => qc.invalidateQueries({ queryKey: ["providers"] });

  const submit = async (v: any) => {
    try {
      if (editing) {
        await api.providers.update(editing.id, v);
      } else {
        await api.providers.create(v);
      }
      message.success(editing ? "Updated" : "Created");
      setOpen(false);
      setEditing(null);
      form.resetFields();
      invalidate();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const toggle = async (p: ProviderConnection, active: boolean) => {
    await api.providers.update(p.id, { is_active: active });
    invalidate();
  };

  const remove = async (id: string) => {
    await api.providers.remove(id);
    message.success("Deleted");
    invalidate();
  };

  return (
    <div>
      <Space style={{ marginBottom: 12, justifyContent: "space-between", width: "100%" }}>
        <h3 style={{ margin: 0 }}>Providers</h3>
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={() => {
            setEditing(null);
            form.resetFields();
            setOpen(true);
          }}
        >
          Tambah
        </Button>
      </Space>
      <Table
        rowKey="id"
        dataSource={q.data?.data || []}
        loading={q.isLoading}
        columns={[
          { title: "Provider", dataIndex: "provider" },
          { title: "Nama", dataIndex: "name", render: (v) => v || "—" },
          { title: "API Key", dataIndex: "api_key", render: (v) => <code>{v}</code> },
          {
            title: "Aktif",
            dataIndex: "is_active",
            render: (v, r) => <Switch checked={v} onChange={(c) => toggle(r, c)} size="small" />,
          },
          { title: "Priority", dataIndex: "priority" },
          {
            title: "Health",
            render: (_, r) =>
              r.rate_limited_until ? <Tag color="red">cooldown</Tag> : <Tag color="green">ok</Tag>,
          },
          {
            title: "Aksi",
            render: (_, r) => (
              <Space>
                <Button
                  size="small"
                  onClick={() => {
                    setEditing(r);
                    form.setFieldsValue({
                      name: r.name,
                      api_key: "",
                      is_active: r.is_active,
                      priority: r.priority,
                    });
                    setOpen(true);
                  }}
                >
                  Edit
                </Button>
                <Popconfirm title="Hapus?" onConfirm={() => remove(r.id)}>
                  <Button size="small" danger>
                    Hapus
                  </Button>
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
      <Modal
        title={editing ? "Edit Provider" : "Tambah Provider"}
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        destroyOnClose
      >
        <Form form={form} layout="vertical" onFinish={submit} initialValues={{ is_active: true, priority: 1 }}>
          <Form.Item name="provider" label="Provider ID" rules={[{ required: true }]}>
            <Input placeholder="openai / claude / gemini / deepseek..." disabled={!!editing} />
          </Form.Item>
          <Form.Item name="name" label="Nama">
            <Input placeholder="opsional" />
          </Form.Item>
          <Form.Item name="api_key" label="API Key" rules={[{ required: !editing }]}>
            <Input placeholder={editing ? "kosongkan = tidak diganti" : "sk-..."} />
          </Form.Item>
          <Space size={32}>
            <Form.Item name="priority" label="Priority">
              <InputNumber min={1} />
            </Form.Item>
            <Form.Item name="is_active" label="Aktif" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </div>
  );
}
