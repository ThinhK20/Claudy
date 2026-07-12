import { useEffect, useState } from "react";
import { AppSidebar, type PageKey } from "@/components/app-sidebar";
import { ThemeProvider } from "@/components/theme-provider";
import { useSettings } from "@/lib/settings-store";
import PromptsPage from "@/pages/PromptsPage";
import TranscriptionPage from "@/pages/TranscriptionPage";
import ProvidersPage from "@/pages/ProvidersPage";
import SettingsPage from "@/pages/SettingsPage";

const PAGES: Record<PageKey, React.ComponentType> = {
  prompts: PromptsPage,
  transcription: TranscriptionPage,
  providers: ProvidersPage,
  settings: SettingsPage,
};

export default function MainApp() {
  const [page, setPage] = useState<PageKey>("prompts");
  const load = useSettings((s) => s.load);

  useEffect(() => {
    load();
  }, [load]);

  const Page = PAGES[page];
  return (
    <ThemeProvider>
      <div className="bg-background text-foreground flex h-screen">
        <AppSidebar page={page} onNavigate={setPage} />
        <main className="flex-1 overflow-y-auto">
          <Page />
        </main>
      </div>
    </ThemeProvider>
  );
}
