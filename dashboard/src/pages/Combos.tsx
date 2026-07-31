import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, Input, Space, Tag, message, Popconfirm, Select } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { api } from "../api/client";

export default function Combos() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["combos"], queryFn: api.combos.list });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm();

  const invalidate = () => qc.invalidateQueries({ queryKey: ["combos"] });

  const modelOptions = (models.data?.data as any[])?.map((m: any) => ({
    value: m.id,
    label: m.id,
  })) || [];

  const create = async (v: any) => {
    try {
      await api.combos.create(v);
      message.success("Combo dibuat");
      setOpen(false);
      form.resetFields();
      invalidate();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const remove = async (id: string) => {
    await api.combos.remove(id);
    invalidate();
  };

  return (
    <div>
      <Space style={{ marginBottom: 12, justifyContent: "space-between", width: "100%" }}>
        <h3 style={{ margin: 0 }}>Combos (fallback chains)</h3>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
          Buat Combo
        </Button>
      </Space>
      <Table
        rowKey="id"
        dataSource={q.data?.data || []}
        loading={q.isLoading}
        columns={[
          { title: "Nama", dataIndex: "name" },
          {
            title: "Chain",
            dataIndex: "models",
            render: (models: string[]) => (
              <Space wrap>
                {models.map((m, i) => (
                  <span key={m}>
                    <Tag color="blue">{m}</Tag>
                    {i < models.length - 1 && <span style={{ color: "#888" }}>→</span>}
                  </span>
                ))}
              </Space>
            ),
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
      <Modal title="Buat Combo" open={open} onCancel={() => setOpen(false)} onOk={() => form.submit()} destroyOnClose>
        <Form form={form} layout="vertical" onFinish={create}>
          <Form.Item name="name" label="Nama Combo" rules={[{ required: true }]}>
            <Input placeholder="smart" />
          </Form.Item>
          <Form.Item name="models" label="Model Chain (urutan fallback)" rules={[{ required: true }]}>
            <Select
              mode="multiple"
              showSearch
              placeholder="gpt-4o → claude-sonnet-4 → gemini-2.5-flash"
              options={modelOptions}
              filterOption={(input, opt) => (opt?.value as string)?.toLowerCase().includes(input.toLowerCase())}
            />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
