import { cn } from "@/lib/utils";

const LEVEL_BARS = 5;

interface LevelBarsProps {
  level: number;
  className?: string;
}

/// A small five-bar microphone level meter driven by the `mic-level` event.
/// Active bars use the --success token; inactive ones use `currentColor`, so it
/// reads correctly on both the dark dictation overlay (which forces the dark
/// tokens) and the themed assistant window.
export function LevelBars({ level, className }: LevelBarsProps) {
  // RMS levels for speech are small; scale up so normal speech lights bars.
  const active = Math.min(LEVEL_BARS, Math.round(level * LEVEL_BARS * 4));
  return (
    <div className={cn("flex items-end gap-0.5", className)}>
      {Array.from({ length: LEVEL_BARS }, (_, i) => (
        <span
          key={i}
          className={cn(
            "w-1 rounded-sm",
            i < active ? "bg-success" : "bg-current opacity-25",
          )}
          style={{ height: `${6 + i * 2}px` }}
        />
      ))}
    </div>
  );
}
