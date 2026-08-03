import { Layout, Menu, Typography, ConfigProvider, theme } from "antd";
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
  DatabaseOutlined,
  PlayCircleOutlined,
  SettingOutlined,
  ReadOutlined,
} from "@ant-design/icons";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, useRouter } from "./router";
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

const PAGES: Record<string, React.ComponentType> = {
  "/": Status,
  "/providers": Providers,
  "/api-keys": ApiKeys,
  "/combos": Combos,
  "/logs": Logs,
  "/analytics": Analytics,
  "/costs": Costs,
  "/webhooks": Webhooks,
  "/cache": CachePage,
  "/mcp": McpPage,
  "/a2a": A2aPage,
  "/batch": BatchPage,
  "/settings": Settings,
  "/docs": Docs,
  "/playground": Playground,
  "/login": Login,
};

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
  { key: "/login", icon: <LoginOutlined />, label: "Login" },
];

function Shell() {
  const { route, navigate } = useRouter();
  const authed = !!getAdminKey();

  if (!authed && route !== "/login") {
    return <Login />;
  }

  const Page = PAGES[route] || Status;

  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Sider width={220} theme="dark" breakpoint="lg" collapsedWidth={0}>
        <div style={{ color: "#fff", padding: 16, fontSize: 16, fontWeight: 700 }}>
          omniroute-rs
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[route]}
          items={MENU}
          onClick={(e) => navigate(e.key)}
        />
      </Sider>
      <Layout>
        <Header style={{ background: "#fff", padding: "0 24px", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Typography.Text strong>{route === "/" ? "Dashboard" : route.slice(1).toUpperCase()}</Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {getAdminKey() ? "admin: terhubung" : "belum login"}
          </Typography.Text>
        </Header>
        <Content style={{ margin: 24 }}>
          <Page />
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
        <RouterProvider>
          <Shell />
        </RouterProvider>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
