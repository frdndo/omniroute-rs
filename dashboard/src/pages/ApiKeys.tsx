import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, Input, Space, Switch, message, Popconfirm, Alert, Typography } from "antd";
import { PlusOutlined, CopyOutlined } from "@ant-design/icons";
import { api } from "../api/client";
import type { ApiKey } from "../api/client";

export default function ApiKeys() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["keys"], queryFn: api.keys.list });
  const [open, setOpen] = useState(false);
  const [created, setCreated] = useState<{ id: string; key: string } | null>(null);
  const [form] = Form.useForm();

  const invalidate = () => qc.invalidateQueries({ queryKey: ["keys"] });

  const create = async (v: any) => {
    try {
      const res = await api.keys.create(v);
      setCreated(res);
      setOpen(false);
      form.resetFields();
      invalidate();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const toggle = async (k: ApiKey, active: boolean) => {
    await api.keys.update(k.id, { is_active: active });
    invalidate();
  };

  const remove = async (id: string) => {
    await api.keys.remove(id);
    invalidate();
  };

  return (
    <div>
      <Space style={{ marginBottom: 12, justifyContent: "space-between", width: "100%" }}>
        <h3 style={{ margin: 0 }}>API Keys (gateway auth)</h3>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => { setCreated(null); setOpen(true); }}>
          Buat Key
        </Button>
      </Space>

      {created && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="Key baru — SALIN SEKARANG (tidak akan tampil lagi)"
          description={
            <Space>
              <code>{created.key}</code>
              <Button
                size="small"
                icon={<CopyOutlined />}
                onClick={() => { navigator.clipboard.writeText(created.key); message.success("Tersalin"); }}
              >
                Salin
              </Button>
            </Space>
          }
        />
      )}

      <Table
        rowKey="id"
        dataSource={q.data?.data || []}
        loading={q.isLoading}
        columns={[
          { title: "Nama", dataIndex: "name", render: (v) => v || "—" },
          { title: "Key", dataIndex: "key", render: (v) => <code>{v}</code> },
          {
            title: "Aktif",
            dataIndex: "is_active",
            render: (v, r) => <Switch checked={v} onChange={(c) => toggle(r, c)} size="small" />,
          },
          {
            title: "Aksi",
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

      <Modal title="Buat API Key" open={open} onCancel={() => setOpen(false)} onOk={() => form.submit()} destroyOnClose>
        <Form form={form} layout="vertical" onFinish={create}>
          <Form.Item name="name" label="Nama" rules={[{ required: true }]}>
            <Input placeholder="client-1" />
          </Form.Item>
          <Typography.Text type="secondary">
            Key otomatis digenerate (sk-...). Muncul sekali setelah dibuat.
          </Typography.Text>
        </Form>
      </Modal>
    </div>
  );
}
