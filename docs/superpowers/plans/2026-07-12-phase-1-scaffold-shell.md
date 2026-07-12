# Claudy Phase 1: Scaffold & Shell — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Tauri 2 + React + shadcn/ui application shell: tray-first app with main window, hidden overlay window, typed settings persistence, and sidebar navigation — the foundation every later phase builds on.

**Architecture:** Single Tauri 2.x process. Rust owns all system integration (tray, windows, settings persistence via the store plugin); the React webview is purely presentational. Two windows: `main` (settings UI) and `overlay` (hidden, transparent pill used from Phase 3). Frontend routes by window label.

**Tech Stack:** Tauri 2.x, React 18 + TypeScript + Vite, Tailwind CSS v4, shadcn/ui, Zustand.

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`

**Roadmap context:** This is plan 1 of 6. Later phases (each gets its own plan doc after the previous lands): 2 Audio+STT, 3 Dictation E2E, 4 AI providers + prompt engine, 5 Management UI, 6 Polish & packaging.

## Global Constraints

- Whisper models must live ONLY in the project-scope `models/` directory (already gitignored) — never AppData. (Phase 2 enforces; nothing in Phase 1 may contradict it.)
- API keys never in JSON — OS keyring only (Phase 4).
- Settings JSON keys are camelCase; Rust struct fields snake_case with `#[serde(rename_all = "camelCase")]`.
- App identifier: `com.claudy.app`. Product name: `Claudy`.
- No telemetry, no network calls in Phase 1.
- Windows 11 is the verification platform.

---

### Task 1: Scaffold Tauri + React + TypeScript app

**Files:**
- Create: entire app skeleton at repo root (`package.json`, `vite.config.ts`, `src/`, `src-tauri/`)
- Modify: `.gitignore` (merge template ignores with existing `models/` entry)

**Interfaces:**
- Produces: working `npm run tauri dev`; `src-tauri/tauri.conf.json` with `productName: "Claudy"`, `identifier: "com.claudy.app"`.

- [ ] **Step 1: Verify prerequisites**

Run: `node --version && rustc --version && cargo --version`
Expected: Node ≥ 20, rustc ≥ 1.77 (MSVC toolchain). If missing, stop and report.

- [ ] **Step 2: Scaffold into a temp subdirectory, then move to repo root**

create-tauri-app refuses non-empty targets, so scaffold into `claudy/` and move contents up:

```bash
cd "D:/Coding/Project/Claudy"
npm create tauri-app@latest claudy -- --template react-ts --manager npm --identifier com.claudy.app --yes
# merge scaffold .gitignore into ours, then move everything up
cat claudy/.gitignore >> .gitignore
rm claudy/.gitignore
mv claudy/* . && rmdir claudy
```

- [ ] **Step 3: Set product name and window title**

In `src-tauri/tauri.conf.json`: `"productName": "Claudy"`, and in the first window entry `"title": "Claudy"`. In `package.json`: `"name": "claudy"`.

- [ ] **Step 4: Install and verify dev build runs**

Run: `npm install && npm run tauri dev`
Expected: window titled "Claudy" opens showing the Vite+React template page. Close it (Ctrl+C the dev process).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: scaffold Tauri 2 + React + TypeScript app"
```

---

### Task 2: Tailwind v4 + shadcn/ui

**Files:**
- Modify: `vite.config.ts`, `tsconfig.json`, `tsconfig.app.json`, `src/index.css` (replace), `src/App.tsx` (replace with placeholder)
- Create: `components.json`, `src/lib/utils.ts`, `src/components/ui/*` (generated)
- Delete: `src/App.css`

**Interfaces:**
- Produces: `@/` path alias; shadcn components `button`, `input`, `label`, `select`, `switch`, `separator`, `tooltip` under `src/components/ui/`; Tailwind classes working.

- [ ] **Step 1: Install Tailwind v4**

```bash
npm install tailwindcss @tailwindcss/vite
```

Replace the entire content of `src/index.css` with:

```css
@import "tailwindcss";
```

- [ ] **Step 2: Configure path alias + vite plugin**

`tsconfig.json` and `tsconfig.app.json` — add inside `compilerOptions`:

```json
"baseUrl": ".",
"paths": { "@/*": ["./src/*"] }
```

`vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(__dirname, "./src") } },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
});
```

(Keep any existing Tauri-specific `server` options from the template, e.g. `hmr`/`watch` settings.)

- [ ] **Step 3: Initialize shadcn/ui and add base components**

```bash
npx shadcn@latest init -y
npx shadcn@latest add button input label select switch separator tooltip
```

Accept defaults (style: default, base color: neutral, CSS variables: yes).

- [ ] **Step 4: Smoke-test a component**

Replace `src/App.tsx` with:

```tsx
import { Button } from "@/components/ui/button";

function App() {
  return (
    <div className="flex h-screen items-center justify-center">
      <Button>Claudy</Button>
    </div>
  );
}

export default App;
```

Delete `src/App.css`. Run: `npm run tauri dev`
Expected: centered shadcn-styled button labeled "Claudy".

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add Tailwind v4 and shadcn/ui"
```

---

### Task 3: Tauri plugins + capabilities

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `package.json`, `src-tauri/capabilities/default.json`

**Interfaces:**
- Produces: all plugins registered on the builder; capability file granting both windows access. Later phases rely on plugin handles being available via the app handle (e.g. `app.global_shortcut()`, `app.store(..)`).

- [ ] **Step 1: Add plugins via Tauri CLI (one command each)**

```bash
npm run tauri add store
npm run tauri add notification
npm run tauri add global-shortcut
npm run tauri add clipboard-manager
npm run tauri add autostart
npm run tauri add dialog
npm run tauri add opener
```

Then the Rust-only single-instance plugin:

```bash
cd src-tauri && cargo add tauri-plugin-single-instance && cd ..
```

- [ ] **Step 2: Register plugins in `src-tauri/src/lib.rs`**

Replace the `run()` builder chain with (remove the generated `greet` command):

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Note: `single_instance` must be the FIRST plugin registered (per Tauri docs).

- [ ] **Step 3: Grant capabilities to both windows**

`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capabilities for main and overlay windows",
  "windows": ["main", "overlay"],
  "permissions": [
    "core:default",
    "store:default",
    "notification:default",
    "global-shortcut:allow-register",
    "global-shortcut:allow-unregister",
    "global-shortcut:allow-is-registered",
    "clipboard-manager:allow-read-text",
    "clipboard-manager:allow-write-text",
    "autostart:allow-enable",
    "autostart:allow-disable",
    "autostart:allow-is-enabled",
    "dialog:default",
    "opener:default"
  ]
}
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `npm run tauri dev`
Expected: app builds (first Rust compile is slow) and window opens with no plugin panics in the console.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: register Tauri plugins and window capabilities"
```

---

### Task 4: Two-window config + label-based frontend routing

**Files:**
- Modify: `src-tauri/tauri.conf.json`, `src/App.tsx`, `src/index.css`
- Create: `src/windows/MainApp.tsx`, `src/windows/OverlayPage.tsx`

**Interfaces:**
- Produces: hidden `overlay` window; `App.tsx` routes by `getCurrentWindow().label`. Phase 3's overlay module shows/hides this window — its label is exactly `"overlay"`.

- [ ] **Step 1: Declare both windows**

In `src-tauri/tauri.conf.json`, replace the `app.windows` array:

```json
"windows": [
  {
    "label": "main",
    "title": "Claudy",
    "width": 980,
    "height": 640,
    "minWidth": 800,
    "minHeight": 560,
    "center": true
  },
  {
    "label": "overlay",
    "title": "Claudy Overlay",
    "width": 300,
    "height": 70,
    "visible": false,
    "transparent": true,
    "alwaysOnTop": true,
    "skipTaskbar": true,
    "decorations": false,
    "resizable": false,
    "shadow": false,
    "focus": false
  }
]
```

- [ ] **Step 2: Route by window label**

`src/App.tsx`:

```tsx
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainApp from "@/windows/MainApp";
import OverlayPage from "@/windows/OverlayPage";

function App() {
  return getCurrentWindow().label === "overlay" ? <OverlayPage /> : <MainApp />;
}

export default App;
```

`src/windows/MainApp.tsx` (placeholder; replaced in Task 7):

```tsx
export default function MainApp() {
  return <div className="p-8 text-lg font-medium">Claudy</div>;
}
```

`src/windows/OverlayPage.tsx`:

```tsx
export default function OverlayPage() {
  return (
    <div className="flex h-screen items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/80 px-4 py-2 text-sm text-white">
        <span className="h-2 w-2 rounded-full bg-red-500" />
        Recording…
      </div>
    </div>
  );
}
```

For overlay transparency, `src/index.css` becomes:

```css
@import "tailwindcss";

html, body, #root {
  background: transparent;
}
```

(The main window gets its opaque background from the `bg-background` class on its root div — Task 7 applies it.)

- [ ] **Step 3: Verify**

Run: `npm run tauri dev`
Expected: main window shows "Claudy"; overlay stays hidden. Temporarily set overlay `"visible": true`, re-run, confirm a borderless transparent pill appears on top of other windows, then set back to `false`.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add main and overlay windows with label-based routing"
```

---

### Task 5: System tray + hide-to-tray

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `tray::create(app: &AppHandle) -> tauri::Result<()>`; menu ids `"open"`, `"toggle_dictation"` (no-op until Phase 3), `"quit"`. Closing the main window hides it; the app only exits via tray Quit.

- [ ] **Step 1: Enable tray feature**

In `src-tauri/Cargo.toml`, ensure: `tauri = { version = "2", features = ["tray-icon"] }`

- [ ] **Step 2: Implement `src-tauri/src/tray.rs`**

```rust
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Claudy").build(app)?;
    let toggle = MenuItemBuilder::with_id("toggle_dictation", "Toggle Dictation").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &toggle])
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("bundled icon").clone())
        .tooltip("Claudy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "toggle_dictation" => { /* wired in Phase 3 */ }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

- [ ] **Step 3: Wire tray + hide-to-tray in `lib.rs`**

Add `mod tray;` at the top of `src-tauri/src/lib.rs`. In the builder chain (before `.run(...)`):

```rust
        .setup(|app| {
            tray::create(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
```

- [ ] **Step 4: Verify manually**

Run: `npm run tauri dev`
Expected: tray icon appears; clicking X hides the window (process stays alive); tray → Open Claudy restores it; tray → Quit exits. Launching a second instance while one runs just refocuses the first (single-instance).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add system tray with hide-to-tray behavior"
```

---

### Task 6: Typed settings service (Rust, TDD) + Zustand hydration

**Files:**
- Create: `src-tauri/src/config.rs`, `src/lib/settings-store.ts`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `package.json`

**Interfaces:**
- Consumes: store plugin from Task 3 (`app.store("settings.json")`).
- Produces:
  - Rust: `Settings` struct (all fields below), `config::load(app: &AppHandle) -> Result<Settings, String>`, `config::save(app: &AppHandle, settings: &Settings) -> Result<(), String>`, commands `get_settings`, `update_settings`.
  - TS: `Settings` interface (camelCase mirror), `useSettings` Zustand store with `load()` and `update(patch)`. All later phases read config through these.

- [ ] **Step 1: Write the Settings struct with its tests (tests are the spec)**

`src-tauri/src/config.rs`:

```rust
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,               // "light" | "dark" | "system"
    pub language: String,            // whisper language code or "auto"
    pub model: String,               // model filename, "" = none selected
    pub mic_device: String,          // "" = system default
    pub dictation_shortcut: String,  // e.g. "Ctrl+Shift+D"
    pub keep_model_warm: bool,
    pub restore_clipboard: bool,
    pub auto_paste: bool,
    pub notifications_enabled: bool,
    pub start_minimized: bool,
    pub models_dir_override: String, // "" = default project models/ dir
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "auto".into(),
            model: String::new(),
            mic_device: String::new(),
            dictation_shortcut: "Ctrl+Shift+D".into(),
            keep_model_warm: true,
            restore_clipboard: true,
            auto_paste: false,
            notifications_enabled: true,
            start_minimized: false,
            models_dir_override: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_privacy_safe() {
        let s = Settings::default();
        assert!(!s.auto_paste, "auto-paste must be opt-in");
        assert!(s.restore_clipboard);
        assert_eq!(s.theme, "system");
        assert_eq!(s.dictation_shortcut, "Ctrl+Shift+D");
    }

    #[test]
    fn deserializes_camel_case_and_fills_missing_fields_with_defaults() {
        let json = serde_json::json!({ "theme": "dark", "autoPaste": true });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.theme, "dark");
        assert!(s.auto_paste);
        assert_eq!(s.language, "auto"); // missing field -> default
    }

    #[test]
    fn serializes_to_camel_case() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert!(v.get("dictationShortcut").is_some());
        assert!(v.get("dictation_shortcut").is_none());
    }
}
```

Add `mod config;` to `lib.rs` so the module compiles.

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test config::`
Expected: 3 tests PASS. If an assertion fails, fix the struct — not the test.

- [ ] **Step 3: Add load/save + commands to `config.rs`**

Append:

```rust
pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get("settings") {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => Ok(Settings::default()),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    store.set("settings", value);
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    load(&app)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    save(&app, &settings)
}
```

In `lib.rs` builder chain, add:

```rust
        .invoke_handler(tauri::generate_handler![
            config::get_settings,
            config::update_settings
        ])
```

Ensure `serde_json` is a dependency in `src-tauri/Cargo.toml` (the template includes it).

- [ ] **Step 4: Frontend settings store**

```bash
npm install zustand
```

`src/lib/settings-store.ts`:

```ts
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  theme: "light" | "dark" | "system";
  language: string;
  model: string;
  micDevice: string;
  dictationShortcut: string;
  keepModelWarm: boolean;
  restoreClipboard: boolean;
  autoPaste: boolean;
  notificationsEnabled: boolean;
  startMinimized: boolean;
  modelsDirOverride: string;
}

interface SettingsState {
  settings: Settings | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  load: async () => {
    set({ settings: await invoke<Settings>("get_settings") });
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ settings: next }); // optimistic
    await invoke("update_settings", { settings: next });
  },
}));
```

- [ ] **Step 5: Verify round-trip manually**

Run: `npm run tauri dev`. In the app's devtools console (right-click → Inspect):

```js
const { invoke } = window.__TAURI__.core;
const s = await invoke("get_settings");           // -> defaults
await invoke("update_settings", { settings: { ...s, theme: "dark" } });
await invoke("get_settings");                     // -> theme: "dark"
```

Restart the app; `get_settings` still returns `theme: "dark"` (persisted).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add typed settings service with store persistence"
```

---

### Task 7: Main window shell — sidebar navigation + theme

**Files:**
- Create: `src/components/app-sidebar.tsx`, `src/components/theme-provider.tsx`, `src/pages/PromptsPage.tsx`, `src/pages/TranscriptionPage.tsx`, `src/pages/ProvidersPage.tsx`, `src/pages/SettingsPage.tsx`
- Modify: `src/windows/MainApp.tsx`, `package.json`

**Interfaces:**
- Consumes: `useSettings` from Task 6.
- Produces: page registry keyed `"prompts" | "transcription" | "providers" | "settings"` — later phases fill these pages in. `ThemeProvider` applies the `dark` class from `settings.theme`.

- [ ] **Step 1: Theme provider**

`src/components/theme-provider.tsx`:

```tsx
import { useEffect } from "react";
import { useSettings } from "@/lib/settings-store";

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const theme = useSettings((s) => s.settings?.theme ?? "system");

  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      root.classList.toggle("dark", dark);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  return <>{children}</>;
}
```

- [ ] **Step 2: Placeholder pages**

Each of the four page files follows this pattern (shown for Prompts; repeat with matching title/description — Transcription: "Models, language and microphone (coming in Phase 2)", Providers: "AI provider configuration (coming in Phase 4)", Settings: "Application preferences (coming in Phase 5)"):

```tsx
// src/pages/PromptsPage.tsx
export default function PromptsPage() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-semibold">Prompts</h1>
      <p className="text-muted-foreground mt-1">
        Create AI prompt shortcuts. (Coming in Phase 5)
      </p>
    </div>
  );
}
```

- [ ] **Step 3: Sidebar + shell**

Install icons if the template didn't: `npm install lucide-react`

`src/components/app-sidebar.tsx`:

```tsx
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { MessageSquareText, Mic, Plug, Settings } from "lucide-react";

export type PageKey = "prompts" | "transcription" | "providers" | "settings";

const NAV: { key: PageKey; label: string; icon: React.ElementType }[] = [
  { key: "prompts", label: "Prompts", icon: MessageSquareText },
  { key: "transcription", label: "Transcription", icon: Mic },
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
```

`src/windows/MainApp.tsx` (replaces Task 4 placeholder):

```tsx
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
```

- [ ] **Step 4: Verify**

Run: `npm run tauri dev`
Expected: sidebar with 4 items; clicking switches pages. In devtools, update theme to `"dark"` via `update_settings` — UI flips dark immediately and persists across restart.

- [ ] **Step 5: Run full Phase 1 verification, then commit**

Checklist: dev build runs; tray open/quit works; close hides to tray; overlay hidden but renders when forced visible; settings persist; theme applies; `cd src-tauri && cargo test` green.

```bash
git add -A
git commit -m "feat: add main window shell with sidebar navigation and theming"
```

---

## Verification (end of Phase 1)

1. `npm run tauri dev` — main window opens with sidebar shell; tray icon present.
2. Close window → hidden to tray; tray Open restores; tray Quit exits; second instance refocuses the first.
3. Settings round-trip survives restart; theme switch (light/dark/system) works live.
4. `cd src-tauri && cargo test` — all config tests pass.
5. Nothing written to project `models/` dir and no network calls occurred (Phase 1 constraint).
