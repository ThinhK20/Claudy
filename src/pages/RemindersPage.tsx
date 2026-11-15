import { useCallback, useEffect, useMemo, useState } from "react";
import { AlarmClock, BellRing, Check, ChevronDown, Clock, Pencil, Plus, Trash2 } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { InlineStatus } from "@/components/inline-status";
import { Page } from "@/components/page";
import { DateTimePicker } from "@/components/reminders/date-time-picker";
import {
  byDueAt,
  formatDue,
  fromDateTimeLocal,
  isOverdue,
  toDateTimeLocal,
} from "@/lib/reminder-format";
import { errorMessage } from "@/lib/error-message";
import {
  completeReminder,
  deleteReminder,
  listReminders,
  onRemindersChanged,
  saveReminder,
  snoozeReminder,
  type Reminder,
} from "@/lib/reminders-api";

/** Offered on anything that has already fired. Kept short on purpose — the
 *  editor covers every other time. */
const SNOOZE_OPTIONS: { label: string; minutes: number }[] = [
  { label: "10 min", minutes: 10 },
  { label: "1 hour", minutes: 60 },
  { label: "Tomorrow", minutes: 60 * 24 },
];

/** Re-render cadence for relative labels, so an upcoming reminder flips to
 *  overdue without the user reopening the page. */
const CLOCK_TICK_MS = 30_000;

export default function RemindersPage() {
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<Reminder | null>(null);
  const [editing, setEditing] = useState<Reminder | null>(null);
  const [now, setNow] = useState(() => new Date());

  const reload = useCallback(async () => {
    try {
      setReminders(await listReminders());
      setError(null);
    } catch (e: unknown) {
      setError(`Couldn't load reminders: ${errorMessage(e)}`);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  // The scheduler fires in the background; without this the page would keep
  // showing a reminder as upcoming after its notification already went out.
  useEffect(() => {
    const unlisten = onRemindersChanged(() => void reload());
    return () => {
      unlisten.then((f) => f());
    };
  }, [reload]);

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => clearInterval(id);
  }, []);

  const mutate = async (op: () => Promise<unknown>) => {
    try {
      await op();
      await reload();
    } catch (e: unknown) {
      setError(errorMessage(e));
    }
  };

  // Needs attention first, then what's coming, then what's been dealt with.
  const { needsAttention, upcoming, done } = useMemo(() => {
    const active = reminders.filter((r) => r.status !== "done");
    return {
      needsAttention: active
        .filter((r) => r.status === "fired" || isOverdue(r.dueAt, now))
        .sort(byDueAt),
      upcoming: active
        .filter((r) => r.status !== "fired" && !isOverdue(r.dueAt, now))
        .sort(byDueAt),
      done: [...reminders.filter((r) => r.status === "done")].sort(byDueAt).reverse(),
    };
  }, [reminders, now]);

  const confirmDelete = () => {
    if (toDelete) void mutate(() => deleteReminder(toDelete.id));
    setToDelete(null);
  };

  // Default an hour out: a reminder due "now" is never what you meant.
  const blank = (): Reminder => ({
    id: "",
    text: "",
    title: "",
    dueAt: fromDateTimeLocal(new Date(Date.now() + 60 * 60 * 1000).toString()),
    createdAt: "",
    firedAt: "",
    status: "pending",
  });

  const section = (label: string, items: Reminder[], attention: boolean) =>
    items.length > 0 && (
      <div className="flex flex-col gap-2">
        <h2 className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
          {label}
        </h2>
        {/* py-0: the rows bring their own vertical padding, so the card's
            would only add a gap above the first and below the last. */}
        <Card className="py-0">
          <CardContent className="p-0">
            {items.map((r) => (
              <ReminderRow
                key={r.id}
                reminder={r}
                now={now}
                attention={attention}
                onSnooze={(minutes) => void mutate(() => snoozeReminder(r.id, minutes))}
                onComplete={() => void mutate(() => completeReminder(r.id))}
                onEdit={() => setEditing(r)}
                onDelete={() => setToDelete(r)}
              />
            ))}
          </CardContent>
        </Card>
      </div>
    );

  return (
    <Page
      title="Reminders"
      description="Open the ask box with the assistant shortcut and switch it to Remind (Shift+Tab) to add one by typing. Claudy notifies you at the time and keeps it here until you mark it done."
    >

      <div className="flex items-center gap-3">
        <Button onClick={() => setEditing(blank())}>
          <Plus className="h-4 w-4" />
          New reminder
        </Button>
      </div>

      {error && <InlineStatus onDismiss={() => setError(null)}>{error}</InlineStatus>}

      <ReminderEditor
        initial={editing}
        onClose={() => setEditing(null)}
        onSaved={() => void reload()}
      />

      {reminders.length === 0 && !error && (
        <Card>
          <CardContent className="text-muted-foreground text-sm">
            No reminders yet.
          </CardContent>
        </Card>
      )}

      {section("Needs attention", needsAttention, true)}
      {section("Upcoming", upcoming, false)}
      {section("Done", done, false)}

      <AlertDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this reminder?</AlertDialogTitle>
            <AlertDialogDescription>
              "{toDelete?.title || toDelete?.text}" will be removed and will not notify
              you. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDelete}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

interface ReminderRowProps {
  reminder: Reminder;
  now: Date;
  attention: boolean;
  onSnooze: (minutes: number) => void;
  onComplete: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

function ReminderRow({
  reminder,
  now,
  attention,
  onSnooze,
  onComplete,
  onEdit,
  onDelete,
}: ReminderRowProps) {
  const fired = reminder.status === "fired";
  const isDone = reminder.status === "done";
  const name = reminder.title || reminder.text;

  return (
    <div className="flex items-center gap-3 border-b px-4 py-3 last:border-b-0">
      {attention ? (
        <AlarmClock className="text-destructive h-4 w-4 shrink-0" aria-hidden="true" />
      ) : (
        <Clock className="text-muted-foreground h-4 w-4 shrink-0" aria-hidden="true" />
      )}
      <div className="min-w-0 flex-1">
        <p
          className={`truncate font-medium ${
            isDone ? "text-muted-foreground line-through" : ""
          }`}
        >
          {name}
        </p>
        <div className="text-muted-foreground flex items-center gap-2 text-sm">
          <span>{formatDue(reminder.dueAt, now)}</span>
          {attention && (
            <Badge variant="destructive">{fired ? "Notified" : "Overdue"}</Badge>
          )}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {attention && <SnoozeMenu onSnooze={onSnooze} />}
        {!isDone && (
          <Button
            variant="ghost"
            size="icon"
            title="Mark as done"
            aria-label={`Mark "${name}" as done`}
            onClick={onComplete}
          >
            <Check className="h-4 w-4" aria-hidden="true" />
          </Button>
        )}
        <Button
          variant="ghost"
          size="icon"
          title="Edit"
          aria-label={`Edit "${name}"`}
          onClick={onEdit}
        >
          <Pencil className="h-4 w-4" aria-hidden="true" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title="Delete"
          aria-label={`Delete "${name}"`}
          onClick={onDelete}
        >
          <Trash2 className="h-4 w-4" aria-hidden="true" />
        </Button>
      </div>
    </div>
  );
}

/** One "Snooze" button instead of three, so a fired row stays readable. */
function SnoozeMenu({ onSnooze }: { onSnooze: (minutes: number) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm">
          <BellRing aria-hidden="true" />
          Snooze
          <ChevronDown aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-44 gap-0.5 p-1">
        {SNOOZE_OPTIONS.map((o) => (
          <Button
            key={o.minutes}
            variant="ghost"
            size="sm"
            className="justify-start"
            onClick={() => {
              setOpen(false);
              onSnooze(o.minutes);
            }}
          >
            {o.minutes >= 60 * 24 ? "Until tomorrow" : `For ${o.label}`}
          </Button>
        ))}
      </PopoverContent>
    </Popover>
  );
}

interface ReminderEditorProps {
  /** null = closed. An entry with an empty id is a new reminder. */
  initial: Reminder | null;
  onClose: () => void;
  onSaved: () => void;
}

/**
 * Inline add/edit form. Editing the text does NOT re-run the time parser — the
 * due time is whatever the picker shows, so a phrase left in the text can never
 * silently override a time the user set by hand.
 */
function ReminderEditor({ initial, onClose, onSaved }: ReminderEditorProps) {
  const [text, setText] = useState("");
  const [when, setWhen] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setText(initial?.text ?? "");
    setWhen(initial ? toDateTimeLocal(initial.dueAt) : "");
    setError(null);
  }, [initial]);

  if (!initial) return null;

  const submit = async () => {
    const trimmed = text.trim();
    const dueAt = fromDateTimeLocal(when);
    if (!trimmed) {
      setError("Type what you want to be reminded about");
      return;
    }
    if (!dueAt) {
      setError("Pick a date and time");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await saveReminder({
        ...initial,
        text: trimmed,
        // Re-title only when the text actually changed, so a parser-stripped
        // title survives an edit that only moved the time.
        title: trimmed === initial.text ? initial.title : "",
        dueAt,
        // Re-arm: an edited reminder should notify again.
        status: "pending",
        firedAt: "",
      });
      onSaved();
      onClose();
    } catch (e: unknown) {
      setError(`Couldn't save the reminder: ${errorMessage(e)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardContent className="flex flex-col gap-3">
        <h2 className="font-medium">{initial.id ? "Edit reminder" : "New reminder"}</h2>
        <Input
          autoFocus
          aria-label="What to be reminded about"
          value={text}
          placeholder="Follow up on this email"
          onChange={(e) => setText(e.target.value)}
        />
        <DateTimePicker value={when} onChange={setWhen} />
        {error && <InlineStatus>{error}</InlineStatus>}
        <div className="flex items-center gap-2">
          <Button onClick={() => void submit()} disabled={saving}>
            Save
          </Button>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
