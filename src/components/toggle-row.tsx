import { useId } from "react";

import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

interface ToggleRowProps {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export function ToggleRow({ label, description, checked, onChange }: ToggleRowProps) {
  // Generated rather than passed in: every call site would otherwise have to
  // invent a unique string, and a duplicate would silently point two rows'
  // labels at the same switch. `useId` is stable across re-renders.
  const switchId = useId();
  const descriptionId = `${switchId}-description`;

  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        {/* Without htmlFor the switch has no accessible name at all, and
            clicking the visible label does nothing. */}
        <Label htmlFor={switchId}>{label}</Label>
        <p id={descriptionId} className="text-muted-foreground text-sm">
          {description}
        </p>
      </div>
      <Switch
        id={switchId}
        aria-describedby={descriptionId}
        checked={checked}
        onCheckedChange={onChange}
      />
    </div>
  );
}
