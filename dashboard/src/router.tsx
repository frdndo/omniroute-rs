// Minimal hash-based router (zero dependencies).
// Replaces react-router-dom: our dashboard is a static SPA with a handful
// of routes, so a ~40-line hash router removes an entire dependency tree
// (react-router-dom + react-router) and their advisories.
import { createContext, useCallback, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

interface RouterCtx {
  route: string;
  navigate: (to: string) => void;
}

const Ctx = createContext<RouterCtx>({ route: "/", navigate: () => {} });

export function useRouter(): RouterCtx {
  return useContext(Ctx);
}

export function RouterProvider({ children }: { children: ReactNode }) {
  const get = () => {
    const h = window.location.hash.slice(1);
    return h.startsWith("/") ? h : "/";
  };
  const [route, setRoute] = useState<string>(get());

  useEffect(() => {
    const onChange = () => setRoute(get());
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);

  const navigate = useCallback((to: string) => {
    if (get() === to) {
      setRoute(to);
    } else {
      window.location.hash = to;
    }
  }, []);

  return <Ctx.Provider value={{ route, navigate }}>{children}</Ctx.Provider>;
}
