import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, Row, Col, Statistic, Table, Button, Modal, Form, Input, InputNumber, Space, message, Popconfirm, Tag, Progress } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { api } from "../api/client";

export default function Costs() {
  const qc = useQueryClient();
  const month = new Date().toISOString().slice(0, 7);
  const report = useQuery({ queryKey: ["costs", month], queryFn: () => api.costs(month), refetchInterval: 10000 });
  const pricing = useQuery({ queryKey: ["pricing"], queryFn: api.pricing.list });
  const budgets = useQuery({ queryKey: ["budgets"], queryFn: api.budgets.list });

  const [priceOpen, setPriceOpen] = useState(false);
  const [budgetOpen, setBudgetOpen] = useState(false);
  const [form] = Form.useForm();
  const [bForm] = Form.useForm();

  const r = report.data;

  const addPrice = async (v: any) => {
    await api.pricing.upsert(v);
    message.success("Pricing disimpan");
    setPriceOpen(false);
    form.resetFields();
    qc.invalidateQueries({ queryKey: ["pricing"] });
  };

  const addBudget = async (v: any) => {
    await api.budgets.upsert({ ...v, month });
    message.success("Budget disimpan");
    setBudgetOpen(false);
    bForm.resetFields();
    qc.invalidateQueries({ queryKey: ["budgets"] });
  };

  return (
    <div>
      <h3>Costs — {month}</h3>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Total Spend" value={r?.total_spend_usd ?? 0} precision={4} prefix="$" />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Total Tokens" value={r?.total_tokens ?? 0} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Prompt Tokens" value={r?.prompt_tokens ?? 0} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic title="Tanpa Pricing" value={r?.missing_pricing_models ?? 0} valueStyle={{ color: (r?.missing_pricing_models ?? 0) > 0 ? "#faad14" : undefined }} />
          </Card>
        </Col>
      </Row>

      <Card title="Spend per Provider/Model" style={{ marginTop: 16 }}>
        <Table
          rowKey={(x: any) => `${x.provider}-${x.model}`}
          size="small"
          dataSource={r?.per_provider || []}
          pagination={false}
          columns={[
            { title: "Provider", dataIndex: "provider" },
            { title: "Model", dataIndex: "model" },
            { title: "Prompt Tokens", dataIndex: "prompt_tokens" },
            { title: "Completion Tokens", dataIndex: "completion_tokens" },
            { title: "Spend ($)", dataIndex: "spend_usd", render: (v: number) => `$${v}` },
            { title: "Pricing", dataIndex: "priced", render: (v: boolean) => (v ? <Tag color="green">ada</Tag> : <Tag color="orange">belum</Tag>) },
          ]}
        />
      </Card>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} lg={12}>
          <Card
            title="Pricing ($/MTok)"
            extra={
              <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setPriceOpen(true)}>
                Tambah
              </Button>
            }
          >
            <Table
              size="small"
              rowKey="id"
              dataSource={pricing.data?.data || []}
              pagination={false}
              columns={[
                { title: "Provider", dataIndex: "provider" },
                { title: "Model", dataIndex: "model", render: (v) => <code>{v}</code> },
                { title: "Input", dataIndex: "input_per_mtok" },
                { title: "Output", dataIndex: "output_per_mtok" },
                {
                  title: "",
                  render: (_, r2) => (
                    <Popconfirm
                      title="Hapus?"
                      onConfirm={async () => {
                        await api.pricing.remove(r2.id);
                        qc.invalidateQueries({ queryKey: ["pricing"] });
                      }}
                    >
                      <Button size="small" danger>
                        Hapus
                      </Button>
                    </Popconfirm>
                  ),
                },
              ]}
            />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card
            title="Budget Bulanan"
            extra={
              <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setBudgetOpen(true)}>
                Set Budget
              </Button>
            }
          >
            {budgets.data?.data?.length ? (
              budgets.data.data
                .filter((b: any) => b.month === month)
                .map((b: any) => {
                  const reportBudget = (r?.budgets || []).find((x: any) => x.provider === b.provider);
                  const used = reportBudget?.used_pct ?? 0;
                  return (
                    <div key={b.id} style={{ marginBottom: 12 }}>
                      <Space style={{ width: "100%", justifyContent: "space-between" }}>
                        <b>{b.provider}</b>
                        <span>
                          ${reportBudget?.spent_usd ?? 0} / ${b.limit_usd}
                        </span>
                      </Space>
                      <Progress percent={used} status={used >= 100 ? "exception" : used >= 80 ? "active" : "normal"} size="small" />
                    </div>
                  );
                })
            ) : (
              <span>Belum ada budget untuk {month}</span>
            )}
          </Card>
        </Col>
      </Row>

      <Modal title="Tambah Pricing" open={priceOpen} onCancel={() => setPriceOpen(false)} onOk={() => form.submit()} destroyOnClose>
        <Form form={form} layout="vertical" onFinish={addPrice}>
          <Form.Item name="provider" label="Provider" rules={[{ required: true }]}>
            <Input placeholder="openai" />
          </Form.Item>
          <Form.Item name="model" label="Model (atau * untuk semua)">
            <Input placeholder="gpt-4o" defaultValue="*" />
          </Form.Item>
          <Space size={24}>
            <Form.Item name="input_per_mtok" label="Input $/MTok" rules={[{ required: true }]}>
              <InputNumber min={0} step={0.1} />
            </Form.Item>
            <Form.Item name="output_per_mtok" label="Output $/MTok" rules={[{ required: true }]}>
              <InputNumber min={0} step={0.1} />
            </Form.Item>
          </Space>
        </Form>
      </Modal>

      <Modal title="Set Budget" open={budgetOpen} onCancel={() => setBudgetOpen(false)} onOk={() => bForm.submit()} destroyOnClose>
        <Form form={bForm} layout="vertical" onFinish={addBudget}>
          <Form.Item name="provider" label="Provider" rules={[{ required: true }]}>
            <Input placeholder="openai" />
          </Form.Item>
          <Form.Item name="limit_usd" label={`Limit ($) untuk ${month}`} rules={[{ required: true }]}>
            <InputNumber min={0} step={5} style={{ width: "100%" }} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
