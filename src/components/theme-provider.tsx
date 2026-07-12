import { useEffect } from "react";
import { useSettings } from "@/lib/settings-store";

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const theme = useSettings((s) => s.settings?.theme ?? "system");

  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      root.classList.toggle("dark", dark);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  return <>{children}</>;
}
