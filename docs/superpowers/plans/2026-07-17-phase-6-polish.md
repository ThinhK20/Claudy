# Phase 6 â€” Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## Context

Phases 1â€“5 are complete and merged to `main`. Phase 6 of the spec (`docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md` line 95) is **Polish**: "theme, autostart, NSIS packaging, macOS/Linux compatibility passes." Per user decision, **autostart is out of scope** â€” do not touch `tauri-plugin-autostart` or the `startMinimized` setting.

**Goal:** Ship the theme picker UI (backend is fully plumbed already), fix the Phase 5 known limitation where live-registered global shortcuts can't be captured by the `ShortcutInput` recorder, make the synthetic-chord code path platform-aware (Cmd on macOS), configure a proper NSIS-only installer, and document cross-platform build prerequisites and limitations.

**Scope decisions (locked with user):**
- **macOS/Linux: code-level pass only.** cfg-based Cmd-vs-Ctrl chord modifier, BUILDING.md platform notes, documented limitations. No build verification (Windows-only machine); macOS/Linux sections are explicitly marked untested.
- **Shortcut capture fix: included.** New `suspend_global_shortcuts` / `resume_global_shortcuts` commands; `ShortcutInput` suspends all global shortcuts while capturing so RegisterHotKey-consumed combos reach the webview.
- **Installer: NSIS only.** `bundle.targets: ["nsis"]` with per-user install mode and bundle metadata.
- **Autostart: skipped entirely** (plugin stays initialized in `lib.rs` as-is; no settings UI, no `startMinimized` behavior change).

**Key design decisions:**
- **Suspend/resume is idempotent and self-healing.** Both commands start with `unregister_all()` (a no-op when nothing is registered in `global-hotkey` 0.8) and clear the `PromptShortcuts` map â€” clearing is load-bearing because `sync_prompts` skips registration for accels already in the map, so stale entries after `unregister_all` would silently skip re-binding. `resume` then re-runs the same registration path as startup via a shared `register_all` helper extracted from `init`.
- **Resume must complete BEFORE `onChange` fires.** `update_settings` with a changed dictation shortcut calls `register_dictation(old, new)`, which unregisters the old combo and hard-errors if it isn't registered. So `ShortcutInput` funnels every capture-exit path (success, Escape, Backspace, blur, unmount) through a single async `endCapture` that awaits `resumeGlobalShortcuts()` before invoking `onChange`.
- **Webview crash mid-capture is an accepted limitation:** shortcuts stay suspended until the app restarts (startup `init` self-heals). Not worth a watchdog.
- **Overlay window gets no ThemeProvider** â€” it uses hardcoded high-contrast colors by design and never loads the settings store.
- **Default dictation shortcut stays `"Ctrl+Shift+D"` on all platforms.** A platform-aware default (`Cmd+Shift+D`) would churn config defaults and tests for an untested platform. Documented as a macOS limitation instead.
- **No new Rust unit tests for the suspend/resume commands** (`AppHandle` is not constructible in unit tests â€” established project constraint); covered by manual E2E instead. The chord-modifier constants in `inject.rs` DO get unit tests (pure consts).

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`
**Roadmap context:** Phase 6 of 6 â€” final phase.

## Global Constraints

- Windows 11 is the dev/verification target; keep code cross-platform-shaped. Use `cfg!(target_os = "macos")` in const initializers (keeps both branches type-checked) rather than `#[cfg]` attributes where possible.
- Rust-core monolith: all logic in Rust; the webview is purely presentational (spec line 23).
- No silent failures for user-triggered actions â€” but suspend/resume failures during capture are deliberately best-effort (a failed suspend just means the old Phase 5 limitation applies for that capture; a failed resume is healed at next app start).
- Run Rust commands from PowerShell (`cargo` is not on Git Bash PATH). Gates: `cd src-tauri; cargo test` all green, `npx tsc --noEmit` clean.
- Commit format: `<type>: <description>`, no attribution footer (globally disabled).
- NSIS config values verified against `node_modules/@tauri-apps/cli/config.schema.json`: `installMode` enum is `"currentUser" | "perMachine" | "both"` (NOT `"perUser"`); there is **no** `shortcuts`/`createDesktopShortcut` key â€” the default NSIS template auto-creates Start-Menu + Desktop shortcuts.

## File Structure

Backend (modify only):
- `src-tauri/src/shortcuts.rs` â€” extract `register_all` from `init`; add `suspend_global_shortcuts` / `resume_global_shortcuts` commands.
- `src-tauri/src/lib.rs` â€” register the 2 new commands.
- `src-tauri/src/inject.rs` â€” `CHORD_MODIFIER` / `CHORD_LABEL` consts, platform-aware `STRAY_MODIFIERS`, rename `send_ctrl_key` â†’ `send_chord_key`, new test module.
- `src-tauri/src/selection.rs` â€” call-site rename.
- `src-tauri/tauri.conf.json` â€” bundle section: NSIS-only targets + metadata.

Frontend (modify only):
- `src/lib/shortcuts-api.ts` â€” `suspendGlobalShortcuts` / `resumeGlobalShortcuts` wrappers.
- `src/components/shortcut-input.tsx` â€” suspend on capture start, resume-then-onChange on every exit path.
- `src/pages/SettingsPage.tsx` â€” new Appearance card with theme Select.

Docs (modify only):
- `docs/BUILDING.md` â€” full rewrite: per-platform prerequisites, release build, platform limitations.

## Existing interfaces you will consume (already implemented â€” do not modify unless a task says so)

- `shortcuts::init(app)` (startup registration), `register_dictation(app, old, new)`, `sync_prompts(app) -> Result<Vec<String>, String>` (skips accels already in the `PromptShortcuts` map), `notify_sync_warnings(app, &warnings)`, `PromptShortcuts(pub Mutex<HashMap<String, String>>)` managed state.
- `config::load(app) -> Result<Settings, String>`; `Settings.theme: String` (default `"system"`), `Settings.dictation_shortcut` (default `"Ctrl+Shift+D"`); `update_settings` re-registers a changed dictation shortcut and re-syncs prompts.
- Frontend: `useSettings` zustand store (optimistic `update`), `src/components/theme-provider.tsx` (already applies `.dark` from `settings?.theme ?? "system"`, honors `prefers-color-scheme`; mounted in `MainApp.tsx` â€” theme picker needs **zero** new plumbing), shadcn `Select` in `src/components/ui/select.tsx`, `Settings` type in `src/lib/settings-store.ts` (`theme: "light" | "dark" | "system"`).
- `inject::send_ctrl_key(c)` call sites: `inject.rs` `insert_text` (`'v'`), `selection.rs` `read` (`'c'`).
- enigo 0.6 `Key` derives `Debug/Copy/Clone/PartialEq/Eq/Hash` â€” usable in const items and test assertions.

Task dependency graph: Tasks 1, 2, 4, 5 are mutually independent; Task 3 depends on Task 2.

---

### Task 1: Theme picker card (SettingsPage)

**Files:**
- Modify: `src/pages/SettingsPage.tsx`

**Steps:**

- [x] **Step 1:** Import the shadcn Select primitives and add a module-level theme list:

```tsx
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const THEMES: { value: Settings["theme"]; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];
```

- [x] **Step 2:** Insert a new Appearance card between the "Dictation shortcut" card and the "Behavior" card:

```tsx
<Card>
  <CardHeader>
    <CardTitle>Appearance</CardTitle>
    <CardDescription>Color theme for the main window</CardDescription>
  </CardHeader>
  <CardContent>
    <div className="flex items-center justify-between gap-4">
      <div>
        <Label>Theme</Label>
        <p className="text-muted-foreground text-sm">
          System follows your OS light/dark preference
        </p>
      </div>
      <Select
        value={settings.theme}
        onValueChange={(v) => void safeUpdate({ theme: v as Settings["theme"] })}
      >
        <SelectTrigger className="w-32">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {THEMES.map((t) => (
            <SelectItem key={t.value} value={t.value}>
              {t.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  </CardContent>
</Card>
```

No other plumbing: `ThemeProvider` already reacts to the zustand store.

- [x] **Step 3: Verify** â€” `npx tsc --noEmit` clean. Manual: switch Systemâ†’Darkâ†’Light in the running app; `.dark` class toggles on `<html>` immediately; restart persists the choice.
- [x] **Step 4: Commit** â€” `feat: add theme picker to settings appearance card`

---

### Task 2: Suspend/resume global shortcuts (backend)

**Files:**
- Modify: `src-tauri/src/shortcuts.rs`
- Modify: `src-tauri/src/lib.rs`

**Steps:**

- [x] **Step 1:** Extract the body of `init` into a shared `register_all(app: &AppHandle)`; `init` delegates to it:

```rust
/// Register the dictation shortcut and all prompt shortcuts from stored
/// settings/prompts. Shared by startup `init` and `resume_global_shortcuts`.
fn register_all(app: &AppHandle) {
    let settings = crate::config::load(app).unwrap_or_default();
    if let Err(e) = register_dictation(app, None, &settings.dictation_shortcut) {
        // Settings may be unreadable at this point: always show.
        crate::notify::send(app, true, &format!("Dictation shortcut unavailable: {e}"));
    }

    match sync_prompts(app) {
        Ok(warnings) => notify_sync_warnings(app, &warnings),
        Err(e) => crate::notify::send(app, true, &format!("Prompt shortcuts unavailable: {e}")),
    }
}

/// Startup registration from settings. A conflict (combo owned by another
/// app) is NON-FATAL: notify and keep running â€” the tray toggle still works.
pub fn init(app: &AppHandle) {
    register_all(app);
}
```

- [x] **Step 2:** Add the two commands:

```rust
/// Unregister every global shortcut while a ShortcutInput recorder is
/// capturing â€” registered combos are consumed by the OS (RegisterHotKey)
/// and never reach the webview, so capture needs them released.
/// Idempotent: `unregister_all` on an empty registry is a no-op. The
/// PromptShortcuts map MUST be cleared too â€” `sync_prompts` skips accels
/// already in the map, so stale entries would make resume silently skip
/// re-binding them.
#[tauri::command]
pub fn suspend_global_shortcuts(app: AppHandle) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    app.state::<PromptShortcuts>().0.lock().unwrap().clear();
    Ok(())
}

/// Re-register everything from stored settings/prompts after a capture
/// ends. Runs the same path as startup, so registration failures surface
/// as notifications, not errors. Safe to call without a prior suspend.
#[tauri::command]
pub fn resume_global_shortcuts(app: AppHandle) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    app.state::<PromptShortcuts>().0.lock().unwrap().clear();
    register_all(&app);
    Ok(())
}
```

- [x] **Step 3:** Register both commands in `lib.rs` `invoke_handler`, after `shortcuts::check_shortcut,`.
- [x] **Step 4: Verify** â€” `cd src-tauri; cargo test` all green (13 existing shortcuts tests unaffected).
- [x] **Step 5: Commit** â€” `feat: add suspend and resume commands for global shortcuts`

---

### Task 3: ShortcutInput capture suspends shortcuts (frontend) â€” depends on Task 2

**Files:**
- Modify: `src/lib/shortcuts-api.ts`
- Modify: `src/components/shortcut-input.tsx`

**Steps:**

- [x] **Step 1:** Append typed wrappers to `shortcuts-api.ts`:

```ts
/** Release every global shortcut so a recorder can capture live combos. */
export const suspendGlobalShortcuts = (): Promise<void> =>
  invoke("suspend_global_shortcuts");

/** Re-register all global shortcuts from stored settings/prompts. */
export const resumeGlobalShortcuts = (): Promise<void> =>
  invoke("resume_global_shortcuts");
```

- [x] **Step 2:** Rework `ShortcutInput` capture lifecycle. Keep the warning `useEffect` unchanged. Add a `capturingRef` so the exit funnel is single-shot; every exit path awaits resume BEFORE `onChange` (load-bearing: `update_settings` unregisters the old dictation combo, which must be registered again by then):

```tsx
const [capturing, setCapturing] = useState(false);
const capturingRef = useRef(false);

// Suspend is best-effort: if it fails, capture still works for combos the
// OS doesn't consume (the pre-fix behavior).
const startCapture = () => {
  if (capturingRef.current) return;
  capturingRef.current = true;
  setCapturing(true);
  void suspendGlobalShortcuts().catch(() => {});
};

// Single-shot exit funnel. Resume must COMPLETE before the caller's
// onChange runs â€” update_settings re-registers the dictation combo and
// errors if the old one isn't currently registered.
const endCapture = async () => {
  if (!capturingRef.current) return;
  capturingRef.current = false;
  setCapturing(false);
  try {
    await resumeGlobalShortcuts();
  } catch {
    // Healed at next app start by shortcuts::init.
  }
};

// Unmount while capturing (e.g. editor dialog closed): restore shortcuts.
useEffect(() => () => { void endCapture(); }, []);
```

`onKeyDown` becomes:

```tsx
const onKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>) => {
  if (!capturing) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") {
    void endCapture();
    return;
  }
  if (e.key === "Backspace" || e.key === "Delete") {
    if (allowClear) void endCapture().then(() => onChange(""));
    return;
  }
  const accel = acceleratorFromEvent(e);
  if (accel) void endCapture().then(() => onChange(accel));
};
```

Button handlers: `onClick={startCapture}`, `onBlur={() => void endCapture()}` (keep `onKeyDown`).

- [x] **Step 3: Verify** â€” `npx tsc --noEmit` clean. Manual E2E in the running app (see end-of-phase Verification for the full checklist): while capturing, pressing the LIVE dictation combo is captured instead of toggling dictation; saving a NEW dictation combo succeeds (proves resume-before-onChange ordering).
- [x] **Step 4: Commit** â€” `feat: suspend global shortcuts while recording a combo`

---

### Task 4: Platform-aware chord modifier (TDD)

**Files:**
- Modify: `src-tauri/src/inject.rs`
- Modify: `src-tauri/src/selection.rs`

**Steps:**

- [x] **Step 1: Write the failing tests** â€” new `#[cfg(test)] mod tests` at the bottom of `inject.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_modifier_matches_the_platform_convention() {
        if cfg!(target_os = "macos") {
            assert_eq!(CHORD_MODIFIER, Key::Meta);
            assert_eq!(CHORD_LABEL, "Cmd");
        } else {
            assert_eq!(CHORD_MODIFIER, Key::Control);
            assert_eq!(CHORD_LABEL, "Ctrl");
        }
    }

    #[test]
    fn stray_modifiers_never_include_the_chord_modifier() {
        assert!(!STRAY_MODIFIERS.contains(&CHORD_MODIFIER));
    }

    #[test]
    fn stray_modifiers_always_release_shift_and_alt() {
        assert!(STRAY_MODIFIERS.contains(&Key::Shift));
        assert!(STRAY_MODIFIERS.contains(&Key::Alt));
    }
}
```

- [x] **Step 2:** Replace the hardcoded Ctrl chord with platform consts (`cfg!` in const initializers keeps both branches type-checked on every platform):

```rust
/// The platform's copy/paste chord modifier: Cmd on macOS, Ctrl elsewhere.
pub(crate) const CHORD_MODIFIER: Key = if cfg!(target_os = "macos") {
    Key::Meta
} else {
    Key::Control
};
pub(crate) const CHORD_LABEL: &str = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };

const STRAY_MODIFIERS: [Key; 3] = if cfg!(target_os = "macos") {
    [Key::Shift, Key::Alt, Key::Control]
} else {
    [Key::Shift, Key::Alt, Key::Meta]
};
```

Rename `send_ctrl_key` â†’ `send_chord_key`, using `CHORD_MODIFIER` for press/release and `CHORD_LABEL` in the error messages. Update the `STRAY_MODIFIERS` doc comment ("Ctrl itself is exempt" â†’ the chord modifier is exempt).

- [x] **Step 3:** Update call sites: `inject.rs` `insert_text` â†’ `send_chord_key('v')`; `selection.rs` `read` â†’ `crate::inject::send_chord_key('c')`.
- [x] **Step 4: Verify** â€” `cd src-tauri; cargo test` all green (3 new tests pass, 2 existing selection tests unaffected). Manual smoke: dictation still pastes; prompt auto-paste/copy still works.
- [x] **Step 5: Commit** â€” `fix: use Cmd as the synthetic chord modifier on macOS`

---

### Task 5: NSIS packaging + BUILDING.md

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `docs/BUILDING.md`

**Steps:**

- [x] **Step 1:** Replace the `bundle` section of `tauri.conf.json`:

```json
"bundle": {
  "active": true,
  "targets": ["nsis"],
  "publisher": "Claudy",
  "copyright": "Copyright Â© 2026 Claudy",
  "category": "Productivity",
  "shortDescription": "Voice dictation and AI text prompts for any app",
  "longDescription": "Claudy is a desktop assistant for voice dictation and AI-powered text prompts. Dictate into any application with local Whisper transcription, and transform selected text with configurable AI prompts triggered by global shortcuts.",
  "windows": {
    "nsis": {
      "installMode": "currentUser",
      "languages": ["English"],
      "displayLanguageSelector": false
    }
  },
  "icon": [
    "icons/32x32.png",
    "icons/128x128.png",
    "icons/128x128@2x.png",
    "icons/icon.icns",
    "icons/icon.ico"
  ]
}
```

- [x] **Step 2:** Rewrite `docs/BUILDING.md` with: Windows prerequisites (existing content), macOS prerequisites (marked untested: Xcode CLT, `brew install cmake`), Linux prerequisites (marked untested: `webkit2gtk-4.1`, `libxdo`, `libayatana-appindicator3`, `librsvg2`, cmake+clang for whisper-rs, `libasound2-dev` for cpal, a Secret Service provider for keyring), Run section, Release build section (`npm run tauri build` â†’ installer at `src-tauri/target/release/bundle/nsis/Claudy_<version>_x64-setup.exe`; unsigned-binary SmartScreen warning note), Platform limitations (macOS: Accessibility permission required for enigo, chords use Cmd, the default `Ctrl+Shift+D` dictation shortcut is not platform-aware; Linux: injection/global shortcuts need an X11 session on Wayland, tray needs appindicator), Whisper models section (existing content).
- [x] **Step 3: Verify** â€” `npm run tauri build` produces the NSIS installer; install it, launch from the Start Menu shortcut, dictation shortcut works, then uninstall cleanly.
- [x] **Step 4: Commit** â€” `feat: configure NSIS-only installer and document platform builds`

---

## End-of-phase Verification

Automated gates (all must pass):
- `cd src-tauri; cargo test` â€” all green.
- `npx tsc --noEmit` â€” clean.
- `npm run tauri build` â€” NSIS installer produced.

Manual E2E (dev app, `npm run tauri dev`):
1. **Theme:** Settings â†’ Appearance: Dark applies immediately (`.dark` on `<html>`); Light removes it; System follows the OS; choice survives restart. Overlay appearance unchanged (hardcoded colors by design).
2. **Capture fix (the Phase 5 limitation):** Settings â†’ Dictation shortcut â†’ click to capture â†’ press the LIVE current combo (e.g. `Ctrl+Shift+D`): it is **captured** and dictation does NOT toggle. Same for an enabled prompt's registered combo inside the prompt editor: captured, prompt does not run.
3. **Ordering:** capture and save a NEW dictation combo â€” save succeeds (no "could not release current shortcut" error), and the new combo works globally afterwards.
4. **Exit paths:** Escape, Backspace (clear), click-away blur, and closing the prompt-editor dialog mid-capture each restore global shortcuts (dictation combo works again immediately after).
5. **Idempotency (CDP, optional):** with `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"`, invoke `suspend_global_shortcuts` twice then `resume_global_shortcuts` twice via `window.__TAURI__.core.invoke` â€” no errors, shortcuts functional afterwards.
6. **Chord smoke:** dictation still pastes text; a prompt with auto-paste still replaces the selection.
7. **Installer:** install the built NSIS setup per-user (no admin prompt), launch from Start Menu, verify tray + dictation, uninstall cleanly.

Known accepted limitations (document, don't fix):
- Webview crash mid-capture leaves shortcuts suspended until app restart.
- macOS/Linux sections of BUILDING.md are untested (no hardware).
- Default dictation shortcut is not platform-aware on macOS.
