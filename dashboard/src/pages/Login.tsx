import { useState } from "react";
import { Card, Form, Input, Button, Typography, message, Divider } from "antd";
import { LockOutlined, KeyOutlined } from "@ant-design/icons";
import { useNavigate } from "react-router-dom";
import { setAdminKey, setGatewayKey, getAdminKey } from "../api/client";

export default function Login() {
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();
  const [form] = Form.useForm();

  const onFinish = async (v: { admin_key?: string; gateway_key?: string }) => {
    setLoading(true);
    try {
      if (v.admin_key) {
        setAdminKey(v.admin_key);
        // probe the admin API
        const res = await fetch("/admin/providers", {
          headers: { Authorization: `Bearer ${v.admin_key}` },
        });
        if (!res.ok) {
          setAdminKey("");
          message.error(`Admin key ditolak (${res.status})`);
          return;
        }
      }
      if (v.gateway_key) setGatewayKey(v.gateway_key);
      message.success("Masuk");
      navigate("/");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div style={{ maxWidth: 420, margin: "60px auto" }}>
      <Typography.Title level={3} style={{ textAlign: "center" }}>
        omniroute-rs Dashboard
      </Typography.Title>
      <Card>
        <Form form={form} onFinish={onFinish} layout="vertical">
          <Form.Item
            name="admin_key"
            label="Admin Key (OMNIROUTE_ADMIN_KEYS)"
            rules={[{ required: true, message: "Admin key wajib" }]}
          >
            <Input.Password prefix={<LockOutlined />} placeholder="sk-admin..." />
          </Form.Item>
          <Divider plain>Opsional</Divider>
          <Form.Item name="gateway_key" label="Gateway Key (untuk Playground)">
            <Input.Password prefix={<KeyOutlined />} placeholder="sk-gateway..." />
          </Form.Item>
          <Button type="primary" htmlType="submit" block loading={loading}>
            Masuk
          </Button>
        </Form>
        {getAdminKey() && (
          <Button style={{ marginTop: 12 }} block onClick={() => navigate("/")}>
            Lanjut dengan key tersimpan
          </Button>
        )}
      </Card>
    </div>
  );
}
