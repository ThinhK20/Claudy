# Phase 5 — Management UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## Context

Phases 1–4 are complete and merged to `main` (scaffold/shell, audio+STT, dictation E2E, AI layer — both spec success criteria met). Phase 5 of the spec (`docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md` line 94) is the **Management UI**: prompt manager, shortcut manager, provider settings, import/export. Every backend capability it surfaces already exists — this phase is mostly frontend, plus three small Rust additions (prompt import/export commands and a shortcut conflict-check command).

**Goal:** Replace the Prompts and Settings placeholder pages with full management UIs (prompt CRUD/search/toggle/duplicate/run-now, JSON import/export, shortcut recorder with live conflict warnings, general behavior toggles) and polish the Providers page so any provider can be configured without switching the active one.

**Architecture:** Three thin Rust additions behind commands (`prompts::merge_imported` + file IO commands, `shortcuts::find_conflict` + `check_shortcut`) keep all logic in the Rust core. The frontend gains two shared components (`ShortcutInput` keyboard recorder, `PromptEditor` dialog), four generated shadcn primitives (dialog, textarea, alert-dialog, tabs), and rewrites of the three management pages on the established patterns (typed invoke wrappers, `useSettings` optimistic updates, commit-on-blur inputs).

**Tech Stack:** No new Rust dependencies, no new Tauri plugins, no capability changes (`tauri-plugin-dialog` + `dialog:default` already present for file pickers). Frontend: existing `@tauri-apps/plugin-dialog`, shadcn/ui, zustand.

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`
**Roadmap context:** Phase 5 of 6 — "Management UI — prompt manager, shortcut manager, provider settings, import/export." Out of scope: theme picker, autostart, packaging, macOS/Linux passes (all Phase 6). `theme` and autostart settings fields already exist but get **no UI this phase**.

**Key design decisions (locked during planning):**
- **Import = upsert by id.** Re-importing your own export is idempotent (0 added, N updated). Entries with a missing/empty id get a fresh uuid and are added. Entries with an empty name or template are skipped with a per-entry reason; an unparseable shortcut is cleared with a warning but the prompt still imports. The export format is a plain JSON array of `Prompt` objects.
- **Shortcut editing is capture-based, warnings don't block.** A `ShortcutInput` component records the pressed combo (letters, digits, F1–F24, Space + at least one modifier) and shows a live conflict warning from the new `check_shortcut` command. Saving a conflicting prompt shortcut is still allowed — the backend's existing sync model treats conflicts as skip-with-notification, and the inline warning mirrors that. Only the dictation shortcut hard-fails (existing `update_settings` behavior when the OS refuses the combo).
- **Duplicate opens the editor with a prefilled copy and clears its shortcut** — otherwise every duplicate would instantly conflict with its source.
- **Run-now** uses the existing `run_prompt` command. With the main window focused, a `{{selected_text}}` prompt will probe Claudy itself and abort with "no text selected" — expected; the button exists mainly for clipboard/date prompts.
- **Providers page becomes tabbed:** edit any provider's base URL/model/key and test it without making it active; an explicit "Set as active" button + badge replaces the select-as-editor coupling.

## Global Constraints

- Windows 11 is the dev/verification target; keep code cross-platform-shaped (no `#[cfg(windows)]` unless unavoidable).
- Rust-core monolith: all logic in Rust; the webview is purely presentational (spec line 23).
- API keys go to the OS credential store only, never into JSON (spec line 26).
- Zero telemetry; the only network traffic is to user-configured AI providers (spec line 72).
- Prompt results go to the clipboard; the original selection is never overwritten — `auto_paste` is opt-in, default off (spec line 58).
- No silent failures: every user-triggered action ends in visible success or visible error (spec line 80).
- Shortcut registration conflicts are surfaced in the shortcut editor UI (spec line 79).
- All frontend-visible Rust types use `#[serde(rename_all = "camelCase")]` (established pattern in `config.rs`).
- Run Rust commands from PowerShell (`cargo` is not on Git Bash PATH). Gates: `cd src-tauri; cargo test` all green, `npx tsc --noEmit` clean.
- Commit format: `<type>: <description>`, no attribution footer (globally disabled).

## File Structure

Backend (modify only):
- `src-tauri/src/prompts.rs` — pure `merge_imported` + `ImportReport`; commands `export_prompts`, `import_prompts`.
- `src-tauri/src/shortcuts.rs` — pure `find_conflict`; command `check_shortcut` + `ShortcutCheck`.
- `src-tauri/src/lib.rs` — register the 3 new commands.

Frontend (create):
- `src/components/ui/{dialog,textarea,alert-dialog,tabs}.tsx` — generated by the shadcn CLI.
- `src/lib/shortcuts-api.ts` — `checkShortcut` wrapper + `ShortcutCheck` type.
- `src/components/shortcut-input.tsx` — keyboard-capture accelerator recorder with live conflict warning.
- `src/components/prompt-editor.tsx` — create/edit dialog.

Frontend (modify):
- `src/lib/ai-api.ts` — `ImportReport` type + `exportPrompts`/`importPrompts` wrappers.
- `src/pages/PromptsPage.tsx` — rewrite: list, search, toggle, run, duplicate, delete, editor, import/export.
- `src/pages/SettingsPage.tsx` — rewrite: dictation shortcut recorder + behavior toggles.
- `src/pages/ProvidersPage.tsx` — rewrite: per-provider tabs + set-active.

## Existing interfaces you will consume (already implemented — do not modify unless a task says so)

- Commands: `list_prompts() -> Vec<Prompt>`, `save_prompt(prompt) -> Prompt` (upsert; empty id = create, validates name/template/shortcut, re-syncs global shortcuts, warnings arrive as OS notifications), `delete_prompt(id)` (re-syncs), `run_prompt(id)` (fire-and-forget), `set_api_key(provider, key)` (empty = remove), `has_api_key(provider) -> bool`, `delete_api_key(provider)`, `test_provider(provider_id) -> String`, `get_settings() -> Settings`, `update_settings(settings)` (hard-fails if a changed dictation shortcut can't be registered; re-syncs prompt shortcuts).
- Rust internals: `prompts::{Prompt, load, save_list, upsert}` (`Prompt { id, name, template, shortcut, enabled }`, serde camelCase, `enabled` defaults true); `shortcuts::parse(accel) -> Result<Shortcut, String>` (pure, works in unit tests); `shortcuts::{sync_prompts, notify_sync_warnings}`; `config::load(app) -> Result<Settings, String>` (`Settings.dictation_shortcut`, `Settings.ai.active_provider`, ...).
- Frontend: `src/lib/ai-api.ts` (`Prompt` type + `listPrompts`/`savePrompt`/`deletePrompt`/`runPrompt`/`hasApiKey`/`setApiKey`/`testProvider`), `src/lib/settings-store.ts` (`useSettings` zustand store with optimistic `update(patch)` and `load()`; `Settings`, `AiSettings`, `ProviderId`, `ProviderSettings` types), shadcn components in `src/components/ui/` (badge, button, card, input, label, progress, select, separator, switch, tooltip), `@tauri-apps/plugin-dialog` JS (`open`, `save` — permission already granted), lucide-react icons.
- Tauri converts command arg names to camelCase on the JS side: `exclude_prompt_id` → `excludePromptId`, `for_dictation` → `forDictation`, `provider_id` → `providerId`.

---

### Task 1: `prompts.rs` — import/export commands (TDD)

**Files:**
- Modify: `src-tauri/src/prompts.rs` (pure merge + `ImportReport` + 2 commands)
- Modify: `src-tauri/src/lib.rs` (register 2 commands)

**Interfaces:**
- Consumes: existing `Prompt`, `load`, `save_list`, `upsert`, `shortcuts::parse`, `shortcuts::{sync_prompts, notify_sync_warnings}`.
- Produces: `ImportReport { added: usize, updated: usize, skipped: Vec<String>, warnings: Vec<String> }` (serde camelCase); pure `merge_imported(existing: Vec<Prompt>, imported: Vec<Prompt>) -> (Vec<Prompt>, ImportReport)`; commands `export_prompts(path: String) -> Result<usize, String>` and `import_prompts(path: String) -> Result<ImportReport, String>`. Task 6's UI calls both commands.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `src-tauri/src/prompts.rs` (the `p(id, name)` helper already exists there):

```rust
    fn importable(id: &str, name: &str) -> Prompt {
        Prompt { id: id.into(), name: name.into(), template: "T".into(), ..Prompt::default() }
    }

    #[test]
    fn merge_upserts_by_id_and_counts_added_vs_updated() {
        let existing = vec![p("a", "A"), p("b", "B")];
        let (list, report) =
            merge_imported(existing, vec![importable("a", "A2"), importable("c", "C")]);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "A2"); // updated in place
        assert_eq!(list[2].id, "c"); // appended
        assert_eq!((report.added, report.updated), (1, 1));
        assert!(report.skipped.is_empty() && report.warnings.is_empty(), "got: {report:?}");
    }

    #[test]
    fn merge_assigns_uuids_to_imported_prompts_without_ids() {
        let (list, report) = merge_imported(vec![], vec![importable("", "N")]);
        assert_eq!(report.added, 1);
        assert!(!list[0].id.is_empty());
    }

    #[test]
    fn merge_skips_entries_missing_name_or_template_with_reasons() {
        let imported = vec![
            importable("x", "  "),                                     // no name
            Prompt { name: "NoTemplate".into(), ..Prompt::default() }, // no template
        ];
        let (list, report) = merge_imported(vec![], imported);
        assert!(list.is_empty());
        assert_eq!(report.skipped.len(), 2);
        assert!(report.skipped[1].contains("NoTemplate"), "got: {:?}", report.skipped);
    }

    #[test]
    fn merge_clears_invalid_shortcuts_but_keeps_the_prompt() {
        let mut bad = importable("x", "Bad");
        bad.shortcut = "NotAKey+Q".into();
        let (list, report) = merge_imported(vec![], vec![bad]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].shortcut, "");
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("Bad"), "got: {:?}", report.warnings);
    }

    #[test]
    fn import_report_serializes_camel_case_with_all_fields() {
        let v = serde_json::to_value(ImportReport::default()).unwrap();
        for field in ["added", "updated", "skipped", "warnings"] {
            assert!(v.get(field).is_some(), "missing field {field}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run (PowerShell): `cd src-tauri; cargo test prompts`
Expected: FAIL to compile — `merge_imported`, `ImportReport` not found.

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/prompts.rs`, add after `remove`:

```rust
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: usize,
    pub updated: usize,
    /// Entries dropped entirely, with a per-entry reason.
    pub skipped: Vec<String>,
    /// Entries imported after an adjustment (e.g. invalid shortcut cleared).
    pub warnings: Vec<String>,
}

/// Pure import semantics: upsert by id so re-importing your own export is
/// idempotent. Empty id = new prompt (uuid assigned). Invalid entries are
/// reported, never silently dropped or silently "fixed".
pub fn merge_imported(existing: Vec<Prompt>, imported: Vec<Prompt>) -> (Vec<Prompt>, ImportReport) {
    let mut list = existing;
    let mut report = ImportReport::default();
    for (i, mut prompt) in imported.into_iter().enumerate() {
        let label = if prompt.name.trim().is_empty() {
            format!("entry {}", i + 1)
        } else {
            format!("\"{}\"", prompt.name)
        };
        if prompt.name.trim().is_empty() {
            report.skipped.push(format!("{label}: name is empty"));
            continue;
        }
        if prompt.template.trim().is_empty() {
            report.skipped.push(format!("{label}: template is empty"));
            continue;
        }
        if !prompt.shortcut.trim().is_empty() {
            if let Err(e) = crate::shortcuts::parse(&prompt.shortcut) {
                report.warnings.push(format!("{label}: shortcut cleared — {e}"));
                prompt.shortcut = String::new();
            }
        }
        if prompt.id.trim().is_empty() {
            prompt.id = uuid::Uuid::new_v4().to_string();
        }
        let is_update = list.iter().any(|existing| existing.id == prompt.id);
        list = upsert(list, prompt);
        if is_update {
            report.updated += 1;
        } else {
            report.added += 1;
        }
    }
    (list, report)
}
```

and add the commands after `delete_prompt`:

```rust
/// Write all prompts to `path` as a pretty-printed JSON array (the same
/// shape `import_prompts` reads). Returns the exported count.
#[tauri::command]
pub fn export_prompts(app: AppHandle, path: String) -> Result<usize, String> {
    let list = load(&app)?;
    let json = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Could not write {path}: {e}"))?;
    Ok(list.len())
}

#[tauri::command]
pub fn import_prompts(app: AppHandle, path: String) -> Result<ImportReport, String> {
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
    let imported: Vec<Prompt> =
        serde_json::from_str(&raw).map_err(|e| format!("Not a valid prompts export: {e}"))?;
    let (list, report) = merge_imported(load(&app)?, imported);
    save_list(&app, &list)?;
    // Imported shortcuts must take effect (or warn) immediately, like save_prompt.
    crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
    Ok(report)
}
```

Register in `lib.rs` `invoke_handler` (after `prompts::delete_prompt,`):

```rust
            prompts::export_prompts,
            prompts::import_prompts,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 5 new `prompts` tests pass; all suites green.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/prompts.rs src-tauri/src/lib.rs
git commit -m "feat: add prompt import and export commands with merge report"
```

---

### Task 2: `shortcuts.rs` — conflict-check command for the editors (TDD)

**Files:**
- Modify: `src-tauri/src/shortcuts.rs` (pure `find_conflict` + `ShortcutCheck` + command)
- Modify: `src-tauri/src/lib.rs` (register 1 command)

**Interfaces:**
- Consumes: existing `parse`, `config::load`, `prompts::load`.
- Produces: pure `find_conflict(accel: &str, taken: &[(String, String)]) -> Result<Option<String>, String>` (Err = invalid accel; `Ok(Some(label))` = collides with `label`); `ShortcutCheck { ok: bool, message: String }` (serde camelCase); command `check_shortcut(accel: String, exclude_prompt_id: Option<String>, for_dictation: bool) -> Result<ShortcutCheck, String>`. Task 3's `ShortcutInput` calls the command (JS args: `accel`, `excludePromptId`, `forDictation`).

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `src-tauri/src/shortcuts.rs`:

```rust
    #[test]
    fn find_conflict_matches_equivalent_accelerator_strings() {
        let taken = vec![("prompt \"Fix\"".to_string(), "Control+Shift+G".to_string())];
        let hit = find_conflict("Ctrl+Shift+G", &taken).unwrap();
        assert_eq!(hit, Some("prompt \"Fix\"".to_string()));
    }

    #[test]
    fn find_conflict_is_none_for_a_free_combo() {
        let taken = vec![("the dictation shortcut".to_string(), "Ctrl+Shift+D".to_string())];
        assert_eq!(find_conflict("Ctrl+Shift+G", &taken).unwrap(), None);
    }

    #[test]
    fn find_conflict_rejects_invalid_accelerators() {
        assert!(find_conflict("NotAKey+Q", &[]).is_err());
    }

    #[test]
    fn find_conflict_ignores_unparseable_taken_entries() {
        let taken = vec![("junk".to_string(), "???".to_string())];
        assert_eq!(find_conflict("Ctrl+Shift+G", &taken).unwrap(), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test shortcuts`
Expected: FAIL to compile — `find_conflict` not found.

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/shortcuts.rs`, add `use serde::Serialize;` to the imports, then append after `notify_sync_warnings`:

```rust
/// Pure: does `accel` collide with any taken binding? Comparison happens on
/// PARSED shortcuts (same rule as `desired_prompt_bindings`), so
/// "Control+Shift+G" and "Ctrl+Shift+G" count as the same combo. Err =
/// `accel` itself is invalid; unparseable `taken` entries are skipped.
pub fn find_conflict(accel: &str, taken: &[(String, String)]) -> Result<Option<String>, String> {
    let shortcut = parse(accel)?;
    for (label, taken_accel) in taken {
        if parse(taken_accel).map(|s| s == shortcut).unwrap_or(false) {
            return Ok(Some(label.clone()));
        }
    }
    Ok(None)
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCheck {
    pub ok: bool,
    pub message: String, // "" when ok
}

/// Live validation for the shortcut editors (spec line 79: conflicts are
/// surfaced in the UI). `exclude_prompt_id` = the prompt being edited;
/// `for_dictation` = the dictation combo itself is being edited (then only
/// prompt shortcuts count as conflicts). A conflict is a WARNING — the
/// existing sync model skips conflicting bindings with a notification, so
/// this never blocks a save.
#[tauri::command]
pub fn check_shortcut(
    app: AppHandle,
    accel: String,
    exclude_prompt_id: Option<String>,
    for_dictation: bool,
) -> Result<ShortcutCheck, String> {
    let settings = crate::config::load(&app)?;
    let prompts = crate::prompts::load(&app)?;
    let mut taken: Vec<(String, String)> = Vec::new();
    if !for_dictation {
        taken.push(("the dictation shortcut".into(), settings.dictation_shortcut));
    }
    let exclude = exclude_prompt_id.unwrap_or_default();
    for p in &prompts {
        if p.enabled && !p.shortcut.trim().is_empty() && p.id != exclude {
            taken.push((format!("prompt \"{}\"", p.name), p.shortcut.clone()));
        }
    }
    Ok(match find_conflict(&accel, &taken) {
        Ok(None) => ShortcutCheck { ok: true, message: String::new() },
        Ok(Some(label)) => ShortcutCheck {
            ok: false,
            message: format!("Already used by {label}"),
        },
        Err(e) => ShortcutCheck { ok: false, message: e },
    })
}
```

Register in `lib.rs` `invoke_handler` (after `prompt_flow::run_prompt,`):

```rust
            shortcuts::check_shortcut,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 4 new `shortcuts` tests pass; all suites green.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/shortcuts.rs src-tauri/src/lib.rs
git commit -m "feat: add shortcut conflict check command for the editors"
```

---

### Task 3: UI primitives — shadcn components, `shortcuts-api.ts`, `ShortcutInput`

No frontend test runner exists in this repo; the gate is `npx tsc --noEmit` plus the Verification section's manual E2E.

**Files:**
- Create: `src/components/ui/dialog.tsx`, `src/components/ui/textarea.tsx`, `src/components/ui/alert-dialog.tsx`, `src/components/ui/tabs.tsx` (CLI-generated)
- Create: `src/lib/shortcuts-api.ts`
- Create: `src/components/shortcut-input.tsx`

**Interfaces:**
- Consumes: `check_shortcut` command (Task 2), shadcn `Button`, `cn`.
- Produces: `checkShortcut(accel, opts) -> Promise<ShortcutCheck>`; `<ShortcutInput value onChange excludePromptId? forDictation? allowClear? />` — captures a combo on keydown, `onChange` fires with the accelerator string (or `""` on clear), renders its own live conflict warning. Tasks 5 and 7 embed it. Dialog/Textarea/AlertDialog/Tabs primitives for Tasks 4, 5 and 8.

- [ ] **Step 1: Generate the shadcn primitives**

Run: `npx shadcn@latest add --yes dialog textarea alert-dialog tabs`
Expected: four new files under `src/components/ui/`. (If the CLI is unavailable offline, copy the four components from ui.shadcn.com matching the import style of the existing `src/components/ui/*.tsx` files — the unified `radix-ui` package is already a dependency.)

- [ ] **Step 2: Create `src/lib/shortcuts-api.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";

export interface ShortcutCheck {
  ok: boolean;
  message: string;
}

/**
 * Validate an accelerator and check it against existing bindings
 * (dictation + enabled prompt shortcuts). Conflicts are warnings, not
 * hard errors — the backend skips conflicting bindings at sync time.
 */
export const checkShortcut = (
  accel: string,
  opts: { excludePromptId?: string; forDictation?: boolean } = {},
): Promise<ShortcutCheck> =>
  invoke("check_shortcut", {
    accel,
    excludePromptId: opts.excludePromptId ?? null,
    forDictation: opts.forDictation ?? false,
  });
```

- [ ] **Step 3: Create `src/components/shortcut-input.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { checkShortcut } from "@/lib/shortcuts-api";
import { cn } from "@/lib/utils";

/** Keys that never terminate a capture on their own. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/**
 * Map a keydown to a Tauri accelerator string, or null when the event is
 * not capturable. Supported main keys: letters, digits, F1–F24, Space —
 * always with at least one modifier (global shortcuts need one).
 */
export function acceleratorFromEvent(e: {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  if (MODIFIERS.has(e.key)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (mods.length === 0) return null;
  let main: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) main = e.code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) main = e.key;
  else if (e.code === "Space") main = "Space";
  if (!main) return null;
  return [...mods, main].join("+");
}

interface ShortcutInputProps {
  value: string;
  onChange: (accel: string) => void;
  /** Id of the prompt being edited, so it doesn't conflict with itself. */
  excludePromptId?: string;
  /** True when editing the dictation combo itself. */
  forDictation?: boolean;
  /** false = an empty binding is not allowed (dictation must stay bound). */
  allowClear?: boolean;
}

export function ShortcutInput({
  value,
  onChange,
  excludePromptId,
  forDictation = false,
  allowClear = true,
}: ShortcutInputProps) {
  const [capturing, setCapturing] = useState(false);
  const [warning, setWarning] = useState("");

  // Live conflict warning for the current value (also validates the
  // pre-existing value when an editor opens).
  useEffect(() => {
    let stale = false;
    if (!value) {
      setWarning("");
      return;
    }
    checkShortcut(value, { excludePromptId, forDictation })
      .then((check) => {
        if (!stale) setWarning(check.ok ? "" : check.message);
      })
      .catch((e: unknown) => {
        if (!stale) setWarning(String(e));
      });
    return () => {
      stale = true;
    };
  }, [value, excludePromptId, forDictation]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      setCapturing(false);
      return;
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      if (allowClear) {
        onChange("");
        setCapturing(false);
      }
      return;
    }
    const accel = acceleratorFromEvent(e);
    if (accel) {
      onChange(accel);
      setCapturing(false);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          onClick={() => setCapturing(true)}
          onKeyDown={onKeyDown}
          onBlur={() => setCapturing(false)}
          className={cn(
            "w-56 justify-start font-mono",
            capturing && "ring-ring ring-2",
            !value && !capturing && "text-muted-foreground",
          )}
        >
          {capturing ? "Press a key combination…" : value || "Not set"}
        </Button>
        {allowClear && value && !capturing && (
          <Button type="button" variant="ghost" size="sm" onClick={() => onChange("")}>
            Clear
          </Button>
        )}
      </div>
      <p className="text-muted-foreground text-xs">
        {capturing
          ? `Esc cancels${allowClear ? ", Backspace clears" : ""}`
          : "Click, then press a combination like Ctrl+Shift+G"}
      </p>
      {warning && <p className="text-destructive text-sm">{warning}</p>}
    </div>
  );
}
```

- [ ] **Step 4: Type-check**

Run: `npx tsc --noEmit` → clean. `cd src-tauri; cargo test` → still green (sanity only).

- [ ] **Step 5: Commit**

```powershell
git add src/components/ui src/lib/shortcuts-api.ts src/components/shortcut-input.tsx
git commit -m "feat: add shortcut recorder component and ui dialog primitives"
```

---

### Task 4: `PromptsPage` — list, search, enable toggle, run, delete

**Files:**
- Modify: `src/pages/PromptsPage.tsx` (replace the placeholder)

**Interfaces:**
- Consumes: `listPrompts`, `savePrompt`, `deletePrompt`, `runPrompt`, `Prompt` from `@/lib/ai-api`; `AlertDialog` (Task 3); existing badge/button/card/input/switch.
- Produces: a working management list. Task 5 adds the editor ("New prompt" button, per-row Edit, duplicate-into-editor); Task 6 adds import/export. The `reload` callback and `mutate` helper defined here are extended by those tasks.

- [ ] **Step 1: Replace `src/pages/PromptsPage.tsx`**

```tsx
import { useCallback, useEffect, useState } from "react";
import { Play, Trash2 } from "lucide-react";
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
import { Switch } from "@/components/ui/switch";
import {
  deletePrompt,
  listPrompts,
  runPrompt,
  savePrompt,
  type Prompt,
} from "@/lib/ai-api";

export default function PromptsPage() {
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [search, setSearch] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<Prompt | null>(null);

  const reload = useCallback(async () => {
    try {
      setPrompts(await listPrompts());
      setError(null);
    } catch (e: unknown) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const query = search.trim().toLowerCase();
  const visible = prompts.filter(
    (p) =>
      !query ||
      p.name.toLowerCase().includes(query) ||
      p.template.toLowerCase().includes(query),
  );

  const mutate = async (op: () => Promise<unknown>) => {
    try {
      await op();
      await reload();
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const toggleEnabled = (p: Prompt) =>
    mutate(() => savePrompt({ ...p, enabled: !p.enabled }));

  const confirmDelete = () => {
    if (toDelete) void mutate(() => deletePrompt(toDelete.id));
    setToDelete(null);
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Prompts</h1>
        <p className="text-muted-foreground mt-1">
          AI prompts that run on selected text via global shortcuts.
        </p>
      </div>

      <div className="flex items-center gap-3">
        <Input
          value={search}
          placeholder="Search prompts…"
          onChange={(e) => setSearch(e.target.value)}
          className="max-w-sm"
        />
      </div>

      {error && <p className="text-destructive text-sm">{error}</p>}

      <Card>
        <CardContent className="p-0">
          {visible.length === 0 && (
            <p className="text-muted-foreground p-6 text-sm">
              {prompts.length === 0 ? "No prompts yet." : "No prompts match your search."}
            </p>
          )}
          {visible.map((p) => (
            <div
              key={p.id}
              className="flex items-center gap-3 border-b px-4 py-3 last:border-b-0"
            >
              <Switch
                checked={p.enabled}
                onCheckedChange={() => void toggleEnabled(p)}
                aria-label={`Enable ${p.name}`}
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{p.name}</span>
                  {p.shortcut && (
                    <Badge variant="secondary" className="font-mono">
                      {p.shortcut}
                    </Badge>
                  )}
                </div>
                <p className="text-muted-foreground truncate text-sm">{p.template}</p>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  title="Run now (uses the current clipboard/selection)"
                  disabled={!p.enabled}
                  onClick={() => void runPrompt(p.id)}
                >
                  <Play className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Delete"
                  onClick={() => setToDelete(p)}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <AlertDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{toDelete?.name}”?</AlertDialogTitle>
            <AlertDialogDescription>
              This removes the prompt and releases its global shortcut. This cannot be
              undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
```

- [ ] **Step 2: Type-check and smoke-test**

Run: `npx tsc --noEmit` → clean. Optionally `npm run tauri dev`: the seeded "Fix grammar & spelling" prompt appears; toggling it off makes `Ctrl+Shift+G` inert (the binding is unregistered live); delete asks for confirmation and releases the shortcut.

- [ ] **Step 3: Commit**

```powershell
git add src/pages/PromptsPage.tsx
git commit -m "feat: add prompt manager list with search, toggle, run and delete"
```

---

### Task 5: `PromptEditor` dialog — create, edit, duplicate-then-edit

**Files:**
- Create: `src/components/prompt-editor.tsx`
- Modify: `src/pages/PromptsPage.tsx` (New-prompt button, per-row Edit + Duplicate buttons, editor wiring)

**Interfaces:**
- Consumes: `savePrompt`, `Prompt` (ai-api), `ShortcutInput` (Task 3), `Dialog`/`Textarea` (Task 3).
- Produces: `EMPTY_PROMPT: Prompt` constant and `<PromptEditor initial onClose onSaved />` — `initial: Prompt | null` (null = closed, `id: ""` = create mode). Task 6 leaves it untouched.

- [ ] **Step 1: Create `src/components/prompt-editor.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ShortcutInput } from "@/components/shortcut-input";
import { savePrompt, type Prompt } from "@/lib/ai-api";

export const EMPTY_PROMPT: Prompt = {
  id: "",
  name: "",
  template: "",
  shortcut: "",
  enabled: true,
};

interface PromptEditorProps {
  /** null = closed. id "" = create mode (also used for duplicate drafts). */
  initial: Prompt | null;
  onClose: () => void;
  onSaved: () => void;
}

export function PromptEditor({ initial, onClose, onSaved }: PromptEditorProps) {
  const [draft, setDraft] = useState<Prompt>(EMPTY_PROMPT);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Re-seed the form every time the dialog opens with a new subject.
  useEffect(() => {
    if (initial) {
      setDraft(initial);
      setError(null);
      setSaving(false);
    }
  }, [initial]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await savePrompt(draft);
      onSaved();
      onClose();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={initial !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{initial?.id ? "Edit prompt" : "New prompt"}</DialogTitle>
          <DialogDescription>
            Placeholders: {"{{selected_text}}"}, {"{{clipboard}}"}, {"{{date}}"},{" "}
            {"{{time}}"}. Unknown placeholders pass through verbatim.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="prompt-name">Name</Label>
            <Input
              id="prompt-name"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              placeholder="Fix grammar & spelling"
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="prompt-template">Template</Label>
            <Textarea
              id="prompt-template"
              rows={6}
              value={draft.template}
              onChange={(e) => setDraft({ ...draft, template: e.target.value })}
              placeholder={"Correct the grammar of the following text:\n\n{{selected_text}}"}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label>Global shortcut</Label>
            <ShortcutInput
              value={draft.shortcut}
              onChange={(accel) => setDraft({ ...draft, shortcut: accel })}
              excludePromptId={draft.id || undefined}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="prompt-enabled">Enabled</Label>
            <Switch
              id="prompt-enabled"
              checked={draft.enabled}
              onCheckedChange={(enabled) => setDraft({ ...draft, enabled })}
            />
          </div>
          {error && <p className="text-destructive text-sm">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => void save()}
            disabled={saving || !draft.name.trim() || !draft.template.trim()}
          >
            {saving ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 2: Wire it into `src/pages/PromptsPage.tsx`**

Merge into the existing `lucide-react` import line and add the component import:

```tsx
import { Copy, Pencil, Play, Plus, Trash2 } from "lucide-react";
import { EMPTY_PROMPT, PromptEditor } from "@/components/prompt-editor";
```

Add editor state after `const [toDelete, ...]`:

```tsx
  const [editing, setEditing] = useState<Prompt | null>(null);
```

Add the duplicate handler after `toggleEnabled` — it drafts a copy into the editor rather than saving immediately, so the user can rename it and pick a shortcut in one motion:

```tsx
  // Draft, not save: the copy opens in the editor. Its shortcut is cleared —
  // it would instantly conflict with its source.
  const duplicate = (p: Prompt) =>
    setEditing({ ...p, id: "", name: `${p.name} (copy)`, shortcut: "" });
```

Add a "New prompt" button next to the search input (inside the existing `div.flex.items-center.gap-3`):

```tsx
        <Button onClick={() => setEditing(EMPTY_PROMPT)}>
          <Plus className="h-4 w-4" />
          New prompt
        </Button>
```

Add Edit and Duplicate buttons in the per-row actions, between the Run and Delete buttons:

```tsx
                <Button
                  variant="ghost"
                  size="icon"
                  title="Edit"
                  onClick={() => setEditing(p)}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Duplicate"
                  onClick={() => duplicate(p)}
                >
                  <Copy className="h-4 w-4" />
                </Button>
```

Render the editor before the closing `</div>` of the page (next to the `AlertDialog`):

```tsx
      <PromptEditor
        initial={editing}
        onClose={() => setEditing(null)}
        onSaved={() => void reload()}
      />
```

- [ ] **Step 3: Type-check and smoke-test**

Run: `npx tsc --noEmit` → clean. Optionally in `npm run tauri dev`: create a prompt with a recorded shortcut → it fires globally immediately (no restart); recording the dictation combo shows "Already used by the dictation shortcut" inline; duplicating opens the editor pre-filled with "(copy)" and no shortcut; editing the seeded prompt pre-fills the form.

- [ ] **Step 4: Commit**

```powershell
git add src/components/prompt-editor.tsx src/pages/PromptsPage.tsx
git commit -m "feat: add prompt editor dialog with shortcut recorder"
```

---

### Task 6: Import/export UI on the Prompts page

**Files:**
- Modify: `src/lib/ai-api.ts` (`ImportReport` + 2 wrappers)
- Modify: `src/pages/PromptsPage.tsx` (Import/Export buttons + report line)

**Interfaces:**
- Consumes: `export_prompts`/`import_prompts` commands (Task 1), `open`/`save` from `@tauri-apps/plugin-dialog` (plugin + permission already installed).
- Produces: `exportPrompts(path) -> Promise<number>`, `importPrompts(path) -> Promise<ImportReport>` in `ai-api.ts`.

- [ ] **Step 1: Append to `src/lib/ai-api.ts`**

```typescript
// --- Import / export (Phase 5) ---

export interface ImportReport {
  added: number;
  updated: number;
  /** Entries dropped entirely, with reasons. */
  skipped: string[];
  /** Entries imported after an adjustment (e.g. invalid shortcut cleared). */
  warnings: string[];
}

/** Write all prompts to `path` as a JSON array; resolves to the count. */
export const exportPrompts = (path: string): Promise<number> =>
  invoke("export_prompts", { path });

/** Merge prompts from a JSON export at `path` (upsert by id). */
export const importPrompts = (path: string): Promise<ImportReport> =>
  invoke("import_prompts", { path });
```

- [ ] **Step 2: Add the buttons to `src/pages/PromptsPage.tsx`**

Add to the imports (merging into existing lines where the module is already imported):

```tsx
import { Copy, Download, Pencil, Play, Plus, Trash2, Upload } from "lucide-react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import {
  deletePrompt,
  exportPrompts,
  importPrompts,
  listPrompts,
  runPrompt,
  savePrompt,
  type Prompt,
} from "@/lib/ai-api";
```

Add state after `const [editing, ...]`:

```tsx
  const [report, setReport] = useState<string | null>(null);
```

Add the handlers after `confirmDelete`:

```tsx
  const JSON_FILTER = [{ name: "JSON", extensions: ["json"] }];

  const doExport = async () => {
    setError(null);
    setReport(null);
    try {
      const path = await saveFile({
        defaultPath: "claudy-prompts.json",
        filters: JSON_FILTER,
      });
      if (!path) return; // user cancelled
      const count = await exportPrompts(path);
      setReport(`Exported ${count} prompt${count === 1 ? "" : "s"}.`);
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const doImport = async () => {
    setError(null);
    setReport(null);
    try {
      const path = await openFile({ multiple: false, filters: JSON_FILTER });
      if (typeof path !== "string") return; // user cancelled
      const r = await importPrompts(path);
      await reload();
      const notes = [...r.warnings, ...r.skipped];
      setReport(
        `Imported: ${r.added} added, ${r.updated} updated.` +
          (notes.length ? ` ${notes.join("; ")}` : ""),
      );
    } catch (e: unknown) {
      setError(String(e));
    }
  };
```

Extend the toolbar row (after the "New prompt" button):

```tsx
        <div className="ml-auto flex items-center gap-2">
          <Button variant="outline" onClick={() => void doImport()}>
            <Upload className="h-4 w-4" />
            Import
          </Button>
          <Button variant="outline" onClick={() => void doExport()}>
            <Download className="h-4 w-4" />
            Export
          </Button>
        </div>
```

Render the report line next to the error line:

```tsx
      {report && <p className="text-muted-foreground text-sm">{report}</p>}
```

- [ ] **Step 3: Type-check and smoke-test**

Run: `npx tsc --noEmit` → clean. Optionally: Export → pick a path → file contains a JSON array of prompts; Import the same file → "0 added, N updated"; import a file with a bad entry → skip reason shown inline; import a non-JSON file → readable error.

- [ ] **Step 4: Commit**

```powershell
git add src/lib/ai-api.ts src/pages/PromptsPage.tsx
git commit -m "feat: add prompt import and export with file dialogs"
```

---

### Task 7: `SettingsPage` — dictation shortcut recorder + behavior toggles

**Files:**
- Modify: `src/pages/SettingsPage.tsx` (replace the placeholder)

**Interfaces:**
- Consumes: `useSettings` (`update` throws when `update_settings` rejects — e.g. the OS refuses the new dictation combo; `load` re-fetches to revert the optimistic state), `ShortcutInput` (Task 3), card/label/switch.
- Produces: the Settings page. Theme and autostart deliberately absent (Phase 6).

- [ ] **Step 1: Replace `src/pages/SettingsPage.tsx`**

```tsx
import { useState } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ShortcutInput } from "@/components/shortcut-input";
import { useSettings, type Settings } from "@/lib/settings-store";

interface ToggleRowProps {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

function ToggleRow({ label, description, checked, onChange }: ToggleRowProps) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <Label>{label}</Label>
        <p className="text-muted-foreground text-sm">{description}</p>
      </div>
      <Switch checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

export default function SettingsPage() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const load = useSettings((s) => s.load);
  const [error, setError] = useState<string | null>(null);

  if (!settings) return null;

  // update() is optimistic; on rejection re-load so the UI shows reality.
  const safeUpdate = async (patch: Partial<Settings>) => {
    setError(null);
    try {
      await update(patch);
    } catch (e: unknown) {
      setError(String(e));
      await load();
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Settings</h1>
        <p className="text-muted-foreground mt-1">Application preferences.</p>
      </div>

      {error && <p className="text-destructive text-sm">{error}</p>}

      <Card>
        <CardHeader>
          <CardTitle>Dictation shortcut</CardTitle>
          <CardDescription>
            Global combination that starts and stops dictation. Applied immediately.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ShortcutInput
            value={settings.dictationShortcut}
            onChange={(accel) => {
              if (accel) void safeUpdate({ dictationShortcut: accel });
            }}
            forDictation
            allowClear={false}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Behavior</CardTitle>
          <CardDescription>How Claudy delivers results and starts up</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <ToggleRow
            label="Notifications"
            description="Show a desktop notification for results and errors"
            checked={settings.notificationsEnabled}
            onChange={(v) => void safeUpdate({ notificationsEnabled: v })}
          />
          <ToggleRow
            label="Restore clipboard after dictation"
            description="Put your previous clipboard content back after text is inserted"
            checked={settings.restoreClipboard}
            onChange={(v) => void safeUpdate({ restoreClipboard: v })}
          />
          <ToggleRow
            label="Auto-paste prompt results"
            description="Replace the selection with the result instead of only copying it. The result always stays on the clipboard."
            checked={settings.autoPaste}
            onChange={(v) => void safeUpdate({ autoPaste: v })}
          />
          <ToggleRow
            label="Start minimized"
            description="Start hidden in the tray instead of opening the main window"
            checked={settings.startMinimized}
            onChange={(v) => void safeUpdate({ startMinimized: v })}
          />
        </CardContent>
      </Card>
    </div>
  );
}
```

- [ ] **Step 2: Type-check and smoke-test**

Run: `npx tsc --noEmit` → clean. Optionally: record a new dictation combo → old combo dead, new one toggles the overlay; record a combo owned by another app → error line appears and the UI reverts to the real value; toggle auto-paste → `settings.json` updates.

- [ ] **Step 3: Commit**

```powershell
git add src/pages/SettingsPage.tsx
git commit -m "feat: add settings page with dictation shortcut recorder and toggles"
```

---

### Task 8: `ProvidersPage` — per-provider tabs with explicit set-active

**Files:**
- Modify: `src/pages/ProvidersPage.tsx` (restructure: tabs select the provider being EDITED; a button/badge controls which is ACTIVE)

**Interfaces:**
- Consumes: everything the current page already uses, plus `Tabs` (Task 3). `test_provider` already accepts any provider id — no backend change.
- Produces: final Phase-5 Providers page.

- [ ] **Step 1: Replace `src/pages/ProvidersPage.tsx`**

Keep the existing `PROVIDERS` metadata array and `TestState` interface exactly as they are; the changes are: drop the `Select` imports/usage, add `Tabs`, add a `selected` state (which provider is being edited) and a set-active button:

```tsx
import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { hasApiKey, setApiKey, testProvider } from "@/lib/ai-api";
import {
  useSettings,
  type AiSettings,
  type ProviderId,
} from "@/lib/settings-store";

interface ProviderMeta {
  id: ProviderId;
  settingsKey: keyof Omit<AiSettings, "activeProvider">;
  label: string;
  defaultBaseUrl: string;
  defaultModel: string;
  keyHint: string;
}

const PROVIDERS: ProviderMeta[] = [
  {
    id: "openai_compatible",
    settingsKey: "openaiCompatible",
    label: "OpenAI-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    keyHint: "Optional for local servers (LM Studio, llama.cpp)",
  },
  {
    id: "ollama",
    settingsKey: "ollama",
    label: "Ollama",
    defaultBaseUrl: "http://localhost:11434",
    defaultModel: "llama3.2",
    keyHint: "Not used by Ollama",
  },
  {
    id: "anthropic",
    settingsKey: "anthropic",
    label: "Anthropic",
    defaultBaseUrl: "https://api.anthropic.com",
    defaultModel: "claude-sonnet-5",
    keyHint: "Required",
  },
  {
    id: "gemini",
    settingsKey: "gemini",
    label: "Google Gemini",
    defaultBaseUrl: "https://generativelanguage.googleapis.com",
    defaultModel: "gemini-2.5-flash",
    keyHint: "Required",
  },
];

interface TestState {
  status: "idle" | "running" | "ok" | "error";
  message: string;
}

export default function ProvidersPage() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  // null = "show the active provider's tab" (until the user picks one).
  const [selected, setSelected] = useState<ProviderId | null>(null);
  const [isKeyStored, setIsKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [test, setTest] = useState<TestState>({ status: "idle", message: "" });

  const activeId = settings?.ai.activeProvider ?? "openai_compatible";
  const selectedId = selected ?? activeId;
  const meta = PROVIDERS.find((p) => p.id === selectedId) ?? PROVIDERS[0];

  useEffect(() => {
    setKeyDraft("");
    setKeyError(null);
    setTest({ status: "idle", message: "" });
    hasApiKey(meta.id)
      .then(setIsKeyStored)
      .catch((e: unknown) => setKeyError(String(e)));
  }, [meta.id]);

  if (!settings) return null;
  const cfg = settings.ai[meta.settingsKey];
  const isActive = meta.id === activeId;

  const patchProvider = (patch: Partial<{ baseUrl: string; model: string }>) =>
    update({ ai: { ...settings.ai, [meta.settingsKey]: { ...cfg, ...patch } } });

  const setActive = () =>
    update({ ai: { ...settings.ai, activeProvider: meta.id } });

  const saveKey = async () => {
    setKeyError(null);
    try {
      await setApiKey(meta.id, keyDraft); // empty draft = remove the key
      setKeyDraft("");
      setIsKeyStored(await hasApiKey(meta.id));
    } catch (e: unknown) {
      setKeyError(String(e));
    }
  };

  const runTest = async () => {
    setTest({ status: "running", message: "" });
    try {
      const reply = await testProvider(meta.id);
      setTest({ status: "ok", message: reply });
    } catch (e: unknown) {
      setTest({ status: "error", message: String(e) });
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Providers</h1>
        <p className="text-muted-foreground mt-1">
          AI provider for prompt shortcuts. Configure any provider; only the active one
          is used.
        </p>
      </div>

      <Tabs value={meta.id} onValueChange={(v) => setSelected(v as ProviderId)}>
        <TabsList>
          {PROVIDERS.map((p) => (
            <TabsTrigger key={p.id} value={p.id} className="gap-2">
              {p.label}
              {p.id === activeId && <Badge variant="secondary">Active</Badge>}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-4">
            <div>
              <CardTitle>{meta.label}</CardTitle>
              <CardDescription>
                Empty fields use the defaults shown as placeholders.
              </CardDescription>
            </div>
            {isActive ? (
              <Badge>Active provider</Badge>
            ) : (
              <Button variant="outline" size="sm" onClick={() => void setActive()}>
                Set as active
              </Button>
            )}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {/* key remounts on provider switch so defaultValue re-seeds; commit
              on blur — per-keystroke updates would write settings.json each key */}
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Base URL</Label>
            <Input
              key={`${meta.id}-baseUrl`}
              defaultValue={cfg.baseUrl}
              placeholder={meta.defaultBaseUrl}
              onBlur={(e) => patchProvider({ baseUrl: e.target.value.trim() })}
            />
          </div>
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Model</Label>
            <Input
              key={`${meta.id}-model`}
              defaultValue={cfg.model}
              placeholder={meta.defaultModel}
              onBlur={(e) => patchProvider({ model: e.target.value.trim() })}
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>API key</CardTitle>
          <CardDescription>
            Stored in the OS credential store, never in a file. {meta.keyHint}.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <Input
              type="password"
              value={keyDraft}
              placeholder={isKeyStored ? "•••••••• (stored)" : "Paste API key"}
              onChange={(e) => setKeyDraft(e.target.value)}
            />
            <Button
              variant="outline"
              size="sm"
              onClick={() => void saveKey()}
              disabled={!keyDraft && !isKeyStored}
            >
              {keyDraft || !isKeyStored ? "Save key" : "Remove key"}
            </Button>
            {isKeyStored && <Badge variant="secondary">Key stored</Badge>}
          </div>
          {keyError && <p className="text-destructive text-sm">{keyError}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Connection test</CardTitle>
          <CardDescription>
            Sends a one-word prompt through the full pipeline for {meta.label}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div>
            <Button onClick={() => void runTest()} disabled={test.status === "running"}>
              {test.status === "running" ? "Testing…" : "Test connection"}
            </Button>
          </div>
          {test.status === "ok" && (
            <p className="text-sm text-green-600">Reply: {test.message}</p>
          )}
          {test.status === "error" && (
            <p className="text-destructive text-sm">{test.message}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
```

- [ ] **Step 2: Type-check and smoke-test**

Run: `npx tsc --noEmit` → clean. Optionally: open a non-active provider's tab, edit its model and test it — the ACTIVE provider is unchanged (check `settings.json`); "Set as active" moves the badge; key status updates per tab.

- [ ] **Step 3: Commit**

```powershell
git add src/pages/ProvidersPage.tsx
git commit -m "feat: rework providers page with per-provider tabs and set-active"
```

---

## Verification (end of Phase 5)

Automated gates first:

- [ ] `cd src-tauri; cargo test` — all suites green (~9 new tests across Tasks 1–2).
- [ ] `npx tsc --noEmit` — clean.
- [ ] `cd src-tauri; cargo build` — no warnings from the touched modules.

Manual E2E (`npm run tauri dev`, Windows 11):

- [ ] **Prompt CRUD round trip** — create a prompt ("Summarize", template with `{{selected_text}}`, recorded shortcut `Ctrl+Shift+S`) → select text in Notepad → `Ctrl+Shift+S` works immediately, no restart. Edit its name → the list updates. Delete it → the shortcut no longer fires.
- [ ] **Enable toggle is live** — toggle the seeded prompt off → `Ctrl+Shift+G` is inert; toggle on → it fires again.
- [ ] **Conflict warning in the editor** — in the prompt editor, record the dictation combo → inline "Already used by the dictation shortcut"; saving is still allowed and produces the existing "Prompt shortcut skipped" notification (binding skipped, app healthy).
- [ ] **Duplicate** — duplicate a prompt → editor opens pre-filled with "… (copy)" and no shortcut.
- [ ] **Search** — filter matches name and template text; clearing shows everything.
- [ ] **Run now** — a clipboard-based prompt (template using `{{clipboard}}`) runs from the Play button and delivers a result notification.
- [ ] **Export/import** — Export to a file → JSON array on disk, no key material. Re-import unchanged → "0 added, N updated". Hand-edit the file (blank one name, break one shortcut) → import reports one skip and one cleared-shortcut warning; the rest arrive and their shortcuts fire.
- [ ] **Import garbage** — import a non-JSON file → readable inline error, prompts unchanged.
- [ ] **Dictation shortcut recorder** — record a new combo in Settings → old combo dead, new combo toggles the overlay; recording an enabled prompt's combo shows the inline warning first.
- [ ] **Settings toggles persist** — flip auto-paste on via UI → prompt result replaces the selection in-place (Phase 4's manual-JSON step now has UI); flip notifications off → success notifications stop; `settings.json` reflects every toggle.
- [ ] **Providers tabs** — configure and test a NON-active provider without switching; "Set as active" swaps the badge and subsequent prompt runs use it; per-tab key status is correct.
- [ ] **Regression** — dictation round trip (`Ctrl+Shift+D`) and the seeded `Ctrl+Shift+G` prompt flow still work end-to-end.

Wrap-up (same convention as Phase 4):

- [ ] Update `.superpowers/sdd/progress.md` — Phase 5 tasks ledger, mark READY.
- [ ] `git push` after your review.
