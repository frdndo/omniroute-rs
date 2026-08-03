import { BrowserRouter, Routes, Route, Navigate, useLocation, useNavigate } from "react-router-dom";
import { Layout, Menu, Typography, Tag, ConfigProvider, theme } from "antd";
import {
  DashboardOutlined,
  ApiOutlined,
  KeyOutlined,
  LinkOutlined,
  FileTextOutlined,
  ExperimentOutlined,
  LoginOutlined,
  BarChartOutlined,
  DollarOutlined,
} from "@ant-design/icons";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import Login from "./pages/Login";
import Status from "./pages/Status";
import Providers from "./pages/Providers";
import ApiKeys from "./pages/ApiKeys";
import Combos from "./pages/Combos";
import Logs from "./pages/Logs";
import Analytics from "./pages/Analytics";
import Costs from "./pages/Costs";
import Playground from "./pages/Playground";
import { getAdminKey } from "./api/client";

const { Sider, Header, Content } = Layout;
const qc = new QueryClient();

const MENU = [
  { key: "/", icon: <DashboardOutlined />, label: "Status" },
  { key: "/providers", icon: <ApiOutlined />, label: "Providers" },
  { key: "/api-keys", icon: <KeyOutlined />, label: "API Keys" },
  { key: "/combos", icon: <LinkOutlined />, label: "Combos" },
  { key: "/logs", icon: <FileTextOutlined />, label: "Logs" },
  { key: "/analytics", icon: <BarChartOutlined />, label: "Analytics" },
  { key: "/costs", icon: <DollarOutlined />, label: "Costs" },
  { key: "/playground", icon: <ExperimentOutlined />, label: "Playground" },
];

function Shell() {
  const location = useLocation();
  const navigate = useNavigate();
  const authed = !!getAdminKey();

  if (!authed && location.pathname !== "/login") {
    return <Navigate to="/login" replace />;
  }

  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Sider theme="dark" width={210}>
        <div style={{ padding: 16, color: "#fff", fontWeight: 700, fontSize: 16 }}>
          omniroute-rs
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={MENU}
          onClick={(e) => navigate(e.key)}
        />
      </Sider>
      <Layout>
        <Header style={{ background: "#fff", padding: "0 24px", display: "flex", alignItems: "center", gap: 12 }}>
          <Typography.Title level={5} style={{ margin: 0, flex: 1 }}>
            OmniRoute Rust Proxy
          </Typography.Title>
          {authed && (
            <Tag color="blue" icon={<LoginOutlined />} style={{ cursor: "pointer" }}
              onClick={() => { localStorage.removeItem("om_admin_key"); navigate("/login"); }}>
              Logout
            </Tag>
          )}
        </Header>
        <Content style={{ margin: 16 }}>
          <Routes>
            <Route path="/login" element={<Login />} />
            <Route path="/" element={<Status />} />
            <Route path="/providers" element={<Providers />} />
            <Route path="/api-keys" element={<ApiKeys />} />
            <Route path="/combos" element={<Combos />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="/analytics" element={<Analytics />} />
            <Route path="/costs" element={<Costs />} />
            <Route path="/playground" element={<Playground />} />
          </Routes>
        </Content>
      </Layout>
    </Layout>
  );
}

export default function App() {
  return (
    <ConfigProvider theme={{ algorithm: theme.darkAlgorithm }}>
      <QueryClientProvider client={qc}>
        <BrowserRouter>
          <Shell />
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
