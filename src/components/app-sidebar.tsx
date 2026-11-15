import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  AlarmClock,
  ChartColumn,
  MessageSquareText,
  Mic,
  Plug,
  Settings,
  Sparkles,
} from "lucide-react";

/** Page ids. Kept stable: the Rust side navigates by these strings, so a
 *  rename here is a label change only. */
export type PageKey =
  | "prompts"
  | "transcription"
  | "assistant"
  | "reminders"
  | "usage"
  | "providers"
  | "settings";

/** Props every main-window page receives. */
export interface PageProps {
  onNavigate: (page: PageKey) => void;
}

export const NAV: { key: PageKey; label: string; icon: React.ElementType }[] = [
  { key: "prompts", label: "Prompts", icon: MessageSquareText },
  { key: "transcription", label: "Dictation", icon: Mic },
  { key: "assistant", label: "Assistant", icon: Sparkles },
  { key: "reminders", label: "Reminders", icon: AlarmClock },
  { key: "usage", label: "Usage", icon: ChartColumn },
  { key: "providers", label: "Providers", icon: Plug },
  { key: "settings", label: "General", icon: Settings },
];

export function AppSidebar({
  page,
  onNavigate,
}: {
  page: PageKey;
  onNavigate: (page: PageKey) => void;
}) {
  return (
    <aside className="bg-sidebar text-sidebar-foreground border-sidebar-border flex w-52 shrink-0 flex-col gap-1 border-r p-3">
      <div className="px-2 py-3 text-lg font-bold">Claudy</div>
      <nav aria-label="Main" className="flex flex-col gap-1">
        {NAV.map(({ key, label, icon: Icon }) => {
          const isActive = page === key;
          return (
            <Button
              key={key}
              variant="ghost"
              onClick={() => onNavigate(key)}
              aria-current={isActive ? "page" : undefined}
              className={cn(
                "relative justify-start gap-2",
                isActive
                  ? "bg-sidebar-accent text-sidebar-accent-foreground hover:bg-sidebar-accent dark:hover:bg-sidebar-accent font-semibold before:bg-sidebar-primary before:absolute before:inset-y-1.5 before:left-0 before:w-0.5 before:rounded-full"
                  : "text-sidebar-foreground/80 hover:bg-sidebar-accent/70 hover:text-sidebar-accent-foreground dark:hover:bg-sidebar-accent/70",
              )}
            >
              <Icon className="h-4 w-4" aria-hidden="true" />
              {label}
            </Button>
          );
        })}
      </nav>
    </aside>
  );
}
