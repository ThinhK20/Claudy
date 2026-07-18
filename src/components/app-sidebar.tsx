import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { MessageSquareText, Mic, Plug, Settings, Sparkles } from "lucide-react";

export type PageKey =
  | "prompts"
  | "transcription"
  | "assistant"
  | "providers"
  | "settings";

const NAV: { key: PageKey; label: string; icon: React.ElementType }[] = [
  { key: "prompts", label: "Prompts", icon: MessageSquareText },
  { key: "transcription", label: "Transcription", icon: Mic },
  { key: "assistant", label: "Assistant", icon: Sparkles },
  { key: "providers", label: "Providers", icon: Plug },
  { key: "settings", label: "Settings", icon: Settings },
];

export function AppSidebar({
  page,
  onNavigate,
}: {
  page: PageKey;
  onNavigate: (page: PageKey) => void;
}) {
  return (
    <aside className="flex w-52 shrink-0 flex-col gap-1 border-r p-3">
      <div className="px-2 py-3 text-lg font-bold">Claudy</div>
      {NAV.map(({ key, label, icon: Icon }) => (
        <Button
          key={key}
          variant="ghost"
          onClick={() => onNavigate(key)}
          className={cn("justify-start gap-2", page === key && "bg-accent")}
        >
          <Icon className="h-4 w-4" />
          {label}
        </Button>
      ))}
    </aside>
  );
}
