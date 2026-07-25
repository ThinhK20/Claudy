import { useEffect, useState } from "react";
import { CalendarIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  formatDayLabel,
  joinDateTimeLocal,
  splitDateTimeLocal,
} from "@/lib/reminder-format";

interface DateTimePickerProps {
  /** Local wall-clock "YYYY-MM-DDTHH:mm" — what `toDateTimeLocal` produces. */
  value: string;
  onChange: (value: string) => void;
}

/**
 * Day in a Calendar popover, clock in a plain field. Replaces
 * `<input type="datetime-local">`, whose picker is drawn by the browser itself
 * and stayed stubbornly light-themed inside a dark app.
 */
export function DateTimePicker({ value, onChange }: DateTimePickerProps) {
  const [open, setOpen] = useState(false);
  const { date, time } = splitDateTimeLocal(value);

  // A time field reports "" until every segment is filled, so committing each
  // keystroke upward would snap the clock to midnight mid-edit. Hold the
  // half-typed value here and only publish a complete one.
  const [draft, setDraft] = useState(time);
  useEffect(() => setDraft(time), [time]);

  const setTime = (next: string) => {
    setDraft(next);
    if (next) onChange(joinDateTimeLocal(date ?? new Date(), next));
  };

  return (
    <div className="flex items-center gap-2">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="outline" className="w-52 justify-start font-normal">
            <CalendarIcon className="h-4 w-4" />
            {date ? formatDayLabel(date) : "Pick a date"}
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={date ?? undefined}
            defaultMonth={date ?? undefined}
            captionLayout="dropdown"
            autoFocus
            onSelect={(picked) => {
              // Clicking the selected day again deselects it; a reminder always
              // needs a date, so treat that as "no change" and just close.
              if (picked) onChange(joinDateTimeLocal(picked, draft));
              setOpen(false);
            }}
          />
        </PopoverContent>
      </Popover>
      <Input
        type="time"
        aria-label="Time"
        value={draft}
        onChange={(e) => setTime(e.target.value)}
        className="w-32 appearance-none [&::-webkit-calendar-picker-indicator]:hidden"
      />
    </div>
  );
}
