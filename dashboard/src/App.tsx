import { BrowserRouter, Routes, Route, Navigate, useLocation, useNavigate } from "react-router";
import { Layout, Menu, Typography, ConfigProvider, theme } from "antd";
import {
  DashboardOutlined,
  ApiOutlined,
  KeyOutlined,
  LinkOutlined,
  FileTextOutlined,
  ExperimentOutlined,
  BarChartOutlined,
  DollarOutlined,
  DatabaseOutlined,
  PlayCircleOutlined,
  SettingOutlined,
  ReadOutlined,
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
import Webhooks from "./pages/Webhooks";
import CachePage from "./pages/Cache";
import McpPage from "./pages/Mcp";
import A2aPage from "./pages/A2a";
import BatchPage from "./pages/Batch";
import Settings from "./pages/Settings";
import Docs from "./pages/Docs";
import Playground from "./pages/Playground";
import { getAdminKey } from "./api/client";

const { Sider, Content, Header } = Layout;

const MENU = [
  { key: "/", icon: <DashboardOutlined />, label: "Status" },
  { key: "/providers", icon: <ApiOutlined />, label: "Providers" },
  { key: "/api-keys", icon: <KeyOutlined />, label: "API Keys" },
  { key: "/combos", icon: <LinkOutlined />, label: "Combos" },
  { key: "/logs", icon: <FileTextOutlined />, label: "Logs" },
  { key: "/analytics", icon: <BarChartOutlined />, label: "Analytics" },
  { key: "/costs", icon: <DollarOutlined />, label: "Costs" },
  { key: "/webhooks", icon: <ApiOutlined />, label: "Webhooks" },
  { key: "/cache", icon: <DatabaseOutlined />, label: "Cache" },
  { key: "/mcp", icon: <ApiOutlined />, label: "MCP" },
  { key: "/a2a", icon: <ApiOutlined />, label: "A2A" },
  { key: "/batch", icon: <PlayCircleOutlined />, label: "Batch" },
  { key: "/settings", icon: <SettingOutlined />, label: "Settings" },
  { key: "/docs", icon: <ReadOutlined />, label: "Docs" },
  { key: "/playground", icon: <ExperimentOutlined />, label: "Playground" },
];

function Shell() {
  const location = useLocation();
  const navigate = useNavigate();
  const authed = !!getAdminKey();

  if (!authed) {
    // JANGAN Navigate kalau sudah di /login — react-router v8 loop
    // diam-diam (Navigate ke lokasi sama) → blank white.
    // Render Login langsung di sini.
    if (location.pathname === "/login") {
      return <Login />;
    }
    return <Navigate to="/login" replace />;
  }

  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Sider width={220} theme="dark" breakpoint="lg" collapsedWidth={0}>
        <div style={{ color: "#fff", padding: 16, fontSize: 16, fontWeight: 700 }}>
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
        <Header style={{ background: "#fff", padding: "0 24px", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Typography.Text strong>{location.pathname === "/" ? "Dashboard" : location.pathname.slice(1).toUpperCase()}</Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {authed ? "admin: terhubung" : "belum login"}
          </Typography.Text>
        </Header>
        <Content style={{ margin: 24 }}>
          <Routes>
            <Route path="/" element={<Status />} />
            <Route path="/providers" element={<Providers />} />
            <Route path="/api-keys" element={<ApiKeys />} />
            <Route path="/combos" element={<Combos />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="/analytics" element={<Analytics />} />
            <Route path="/costs" element={<Costs />} />
            <Route path="/webhooks" element={<Webhooks />} />
            <Route path="/cache" element={<CachePage />} />
            <Route path="/mcp" element={<McpPage />} />
            <Route path="/a2a" element={<A2aPage />} />
            <Route path="/batch" element={<BatchPage />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/docs" element={<Docs />} />
            <Route path="/playground" element={<Playground />} />
            <Route path="/login" element={<Login />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </Content>
      </Layout>
    </Layout>
  );
}

const queryClient = new QueryClient();

export default function App() {
  return (
    <ConfigProvider theme={{ algorithm: theme.darkAlgorithm }}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <Shell />
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
