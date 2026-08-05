import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, Input, InputNumber, Switch, Space, Tag, message, Popconfirm, Tabs, Card, Typography, Alert } from "antd";
import { PlusOutlined, RocketOutlined } from "@ant-design/icons";
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
    <Tabs
      defaultActiveKey="mine"
      items={[
        {
          key: "mine",
          label: "Provider Saya",
          children: (
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
          ),
        },
        { key: "free", label: "Free (Tanpa Bayar)", children: <FreeTab /> },
      ]}
    />
  );
}

function FreeTab() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["free-providers"], queryFn: () => api.freeProviders.list() });
  const [adding, setAdding] = useState<any>(null);
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);

  const doAdd = async () => {
    if (!adding) return;
    if (adding.category === "apikey" && !keyInput.trim()) {
      message.error("API key wajib diisi untuk kategori ini");
      return;
    }
    setBusy(true);
    try {
      await api.freeProviders.add(adding.id, { api_key: keyInput.trim() || undefined });
      message.success(`${adding.name} ditambahkan!`);
      setAdding(null);
      setKeyInput("");
      qc.invalidateQueries({ queryKey: ["free-providers"] });
      qc.invalidateQueries({ queryKey: ["providers"] });
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setBusy(false);
    }
  };

  if (q.isLoading) return <Typography.Text type="secondary">Loading...</Typography.Text>;
  if (q.isError) {
    return (
      <Alert
        type="error"
        showIcon
        message="Gagal memuat free providers"
        description={String(q.error instanceof Error ? q.error.message : q.error)}
      />
    );
  }
  if (!q.data?.data?.length) {
    return <Alert type="info" showIcon message="Belum ada data" description="Pastikan sidecar proxy sudah di-rebuild (fitur free provider ada di binary proxy, bukan app shell)." />;
  }

  return (
    <div>
      <Typography.Paragraph type="secondary" style={{ marginBottom: 16 }}>
        Provider free-tier bawaan (parity OmniRoute). Kategori <Tag>noauth</Tag> langsung jalan tanpa
        key; <Tag>apikey</Tag> butuh API key gratis (link daftar disediakan).
      </Typography.Paragraph>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))", gap: 12 }}>
        {(q.data?.data || []).map((p) => (
          <Card
            key={p.id}
            size="small"
            title={
              <Space>
                <RocketOutlined style={{ color: p.category === "noauth" ? "#52c41a" : "#1677ff" }} />
                {p.name}
              </Space>
            }
            extra={<Tag color={p.installed ? "green" : "default"}>{p.installed ? "Terpasang" : p.category}</Tag>}
          >
            <Typography.Paragraph style={{ fontSize: 12, marginBottom: 8 }}>{p.free_note}</Typography.Paragraph>
            <Typography.Text type="secondary" style={{ fontSize: 11, display: "block", marginBottom: 4 }}>
              {p.auth_hint}
            </Typography.Text>
            {p.api_key_url && (
              <Typography.Link href={p.api_key_url} target="_blank" style={{ fontSize: 11 }}>
                Ambil API key →
              </Typography.Link>
            )}
            <div style={{ marginTop: 8 }}>
              <Tag style={{ fontSize: 10 }}>{p.base_url}</Tag>
              <Space size={4} wrap style={{ marginTop: 4 }}>
                {p.models.map((m: string) => (
                  <Tag key={m} style={{ fontSize: 10 }}>
                    {m}
                  </Tag>
                ))}
              </Space>
            </div>
            {!p.installed && (
              <Button size="small" type="primary" style={{ marginTop: 10 }} onClick={() => setAdding(p)}>
                Add
              </Button>
            )}
          </Card>
        ))}
      </div>
      <Modal
        title={`Tambah ${adding?.name || ""}`}
        open={!!adding}
        onCancel={() => setAdding(null)}
        onOk={doAdd}
        confirmLoading={busy}
        okText="Tambahkan"
      >
        {adding?.category === "apikey" ? (
          <Form layout="vertical">
            <Form.Item label="API Key (gratis)" required>
              <Input.Password value={keyInput} onChange={(e) => setKeyInput(e.target.value)} placeholder="sk-..." />
            </Form.Item>
            {adding.api_key_url && (
              <Typography.Link href={adding.api_key_url} target="_blank">
                Belum punya? Daftar & ambil key di sini →
              </Typography.Link>
            )}
          </Form>
        ) : (
          <Typography.Text>Provider noauth — langsung aktif tanpa API key.</Typography.Text>
        )}
      </Modal>
    </div>
  );
}
