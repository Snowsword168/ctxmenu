# ctxmenu — Context Menu Manager for Windows 10/11

A single-file GUI utility written in Rust + egui for managing the Windows Explorer right-click context menu and the "Open with" list. Bilingual UI (English / 中文, toggle in the top bar; the choice is saved to `ctxmenu.ini` next to the exe).

## Features

- **Scan** context menu entries in both HKLM and HKCU across 6 locations (all files, files & folders, folders, background/desktop, generic folders, drives), covering both kinds of entries:
  - `shell\<verb>` classic menu commands
  - `shellex\ContextMenuHandlers` third-party COM extensions (archivers, cloud drives, antivirus, ...)
- **Enable / disable** (reversible, nothing is deleted):
  - menu commands: sets/removes the `LegacyDisable` value
  - shell extensions: adds/removes the CLSID in the `Shell Extensions\Blocked` list
- **Delete** with automatic `.reg` backup exported to `ctxmenu_backups\deleted\` first — double-click the backup file to restore
- **Add custom menu items** (written to HKCU, current user only) with optional icon and "show only while Shift is held"; key names are prefixed with `ctxmenu.` for easy identification
- **Full backup**: export every scanned registry location as `.reg` files in one click
- **"Open with" list manager** (separate tab): scan any number of extensions at once (space/comma separated, with a "common text types" preset) across *all five* registration channels —
  - `HKCU\...\Explorer\FileExts\<ext>\OpenWithList` (recent apps MRU; removal also fixes up `MRUList`)
  - `FileExts\<ext>\OpenWithProgids` and `Software\Classes\<ext>\OpenWithProgids` (HKLM/HKCU)
  - `Software\Classes\<ext>\OpenWithList`, `SystemFileAssociations\<ext>` and perceived types (e.g. `text`)
  - `Applications\<exe>\SupportedTypes` self-registration
  - `RegisteredApplications` → `Capabilities\FileAssociations` (how browsers and AI assistants register dozens of extensions at once)
- **Block an app from "Open with" everywhere**: VS Code–style apps register one ProgID per extension (`Foo.txt`, `Foo.md`, ...); blocking marks the whole ProgID family (plus the `Applications` key) with `NoOpenWith` so the app disappears from "Open with" for every file type — fully reversible, nothing is deleted
- **Group-by-app view**: one row per application with a single Block/Unblock button; sortable columns
- **Restart Explorer** button to apply changes immediately

## Download

Grab `ctxmenu-*-windows-x64.exe` from the [Releases](../../releases) page and run it from any writable folder. No installation, no runtime dependencies (Windows 10/11). The app creates `ctxmenu.ini` (language setting) and `ctxmenu_backups\` (registry backups) next to the exe.

The exe embeds a `requireAdministrator` manifest, so every launch shows a UAC prompt — this is required to modify system-wide (HKLM) entries.

## Build from source

```powershell
cargo build --release              # output: target\release\ctxmenu.exe (~3 MB)
cargo test --no-default-features   # tests must disable the admin manifest, or the test process cannot start
```

To publish a release: push a `v*` tag (e.g. `git tag v0.1.0 && git push origin v0.1.0`) and GitHub Actions will build the exe and attach it to a GitHub Release (see `.github/workflows/release.yml`).

## Notes

- Disabling a shell extension works per CLSID: if the same extension appears in several locations (files, folders, ...) it is toggled everywhere at once
- Some display names are MUI indirect strings (e.g. `@shell32.dll,-8506`) and are shown as-is
- Changes apply to newly opened Explorer windows; use the "Restart Explorer" button for stubborn cases

## License

[MIT](LICENSE)
