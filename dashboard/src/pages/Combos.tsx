import { useState, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Table, Button, Modal, Form, Input, Space, Tag, message, Popconfirm, Select, Progress } from "antd";
import { PlusOutlined, ExperimentOutlined } from "@ant-design/icons";
import { api } from "../api/client";
import { buildModelGroups, modelFilterOption } from "../utils/modelGroups";

export default function Combos() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["combos"], queryFn: api.combos.list });
  const models = useQuery({ queryKey: ["models"], queryFn: api.models });
  const configured = useQuery({ queryKey: ["providers"], queryFn: api.providers.list });
  const [open, setOpen] = useState(false);
  const [autoOpen, setAutoOpen] = useState(false);
  const [autoModel, setAutoModel] = useState<string>();
  const [form] = Form.useForm();
  const autoPreview = useQuery({
    queryKey: ["auto-combo", autoModel],
    queryFn: () => api.autoCombo.preview(autoModel!),
    enabled: !!autoModel,
  });

  const invalidate = () => qc.invalidateQueries({ queryKey: ["combos"] });

  const cfgSet = useMemo(
    () => new Set((configured.data?.data as any[])?.map((p: any) => p.provider) ?? []),
    [configured.data]
  );
  const modelGroups = useMemo(
    () => buildModelGroups((models.data?.data as any[]) ?? [], cfgSet),
    [models.data, cfgSet]
  );

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

  const createAuto = async () => {
    if (!autoModel) return;
    try {
      const r = await api.autoCombo.create(autoModel);
      message.success(`Combo "${r.combo.name}" dibuat: ${r.combo.models.join(" → ")}`);
      setAutoOpen(false);
      setAutoModel(undefined);
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
        <Space>
          <Button icon={<ExperimentOutlined />} onClick={() => setAutoOpen(true)}>
            ⚡ Auto Combo
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
            Buat Combo
          </Button>
        </Space>
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
              options={modelGroups}
              filterOption={modelFilterOption}
            />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title="⚡ Auto Combo — ranked dari telemetry"
        open={autoOpen}
        onCancel={() => {
          setAutoOpen(false);
          setAutoModel(undefined);
        }}
        onOk={createAuto}
        okText={`Buat combo auto-${autoModel ?? ""}`}
        okButtonProps={{ disabled: !autoModel || autoPreview.isLoading }}
        width={640}
        destroyOnClose
      >
        <Select
          showSearch
          style={{ width: "100%", marginBottom: 12 }}
          placeholder="Pilih model — ranking provider otomatis"
          value={autoModel}
          onChange={setAutoModel}
          options={modelGroups}
          filterOption={modelFilterOption}
        />
        {autoPreview.isLoading && <Progress percent={100} status="active" size="small" />}
        {autoPreview.data && (
          <div>
            <Space style={{ marginBottom: 8 }}>
              <Tag color="purple">Chain: {autoPreview.data.chain.join(" → ")}</Tag>
            </Space>
            <Table
              rowKey="provider"
              size="small"
              pagination={false}
              dataSource={autoPreview.data.ranking || []}
              columns={[
                {
                  title: "#",
                  width: 36,
                  render: (_, __, i) => i + 1,
                },
                { title: "Provider", dataIndex: "provider", render: (v) => <Tag>{v}</Tag> },
                { title: "Score", dataIndex: "score", width: 70, render: (v) => <b>{v.toFixed(2)}</b> },
                {
                  title: "Health",
                  dataIndex: "health",
                  width: 60,
                  render: (v) => `${Math.round(v * 100)}%`,
                },
                { title: "Req", dataIndex: "requests", width: 60 },
                {
                  title: "Error",
                  dataIndex: "error_rate",
                  width: 70,
                  render: (v) => <Tag color={v > 0.3 ? "red" : "green"}>{Math.round(v * 100)}%</Tag>,
                },
                { title: "Latency", dataIndex: "avg_duration_ms", width: 90, render: (v) => `${Math.round(v)}ms` },
                {
                  title: "Status",
                  render: (_: any, r: any) =>
                    r.connected ? <Tag color="blue">connected</Tag> : <Tag>noauth</Tag>,
                },
              ]}
            />
          </div>
        )}
      </Modal>
    </div>
  );
}
