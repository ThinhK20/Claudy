import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InlineStatus } from "@/components/inline-status";
import { useSettings } from "@/lib/settings-store";

interface PageProps {
  title: string;
  description: React.ReactNode;
  /** Rendered beside the heading, e.g. a range picker. */
  actions?: React.ReactNode;
  children?: React.ReactNode;
}

/** Every main-window page: heading, description, and a readable column. */
export function Page({ title, description, actions, children }: PageProps) {
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 p-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">{title}</h1>
          <p className="text-muted-foreground mt-1">{description}</p>
        </div>
        {actions}
      </div>
      {children}
    </div>
  );
}

/**
 * Shown instead of a settings-backed page until the settings arrive: a light
 * skeleton while loading, or the error with Retry when the load failed —
 * never a blank pane.
 */
export function SettingsUnavailable({ title, description }: Omit<PageProps, "children">) {
  const loadError = useSettings((s) => s.loadError);
  const loading = useSettings((s) => s.loading);
  const load = useSettings((s) => s.load);

  return (
    <Page title={title} description={description}>
      {loadError && !loading ? (
        <div className="flex flex-col items-start gap-3">
          <InlineStatus>Couldn't load your settings: {loadError}</InlineStatus>
          <Button variant="outline" size="sm" onClick={() => void load().catch(() => {})}>
            <RotateCw aria-hidden="true" />
            Retry
          </Button>
        </div>
      ) : (
        <div aria-busy="true" aria-label="Loading settings" className="flex flex-col gap-6">
          {[0, 1].map((i) => (
            <div key={i} className="bg-muted/60 h-36 rounded-xl motion-safe:animate-pulse" />
          ))}
        </div>
      )}
    </Page>
  );
}
