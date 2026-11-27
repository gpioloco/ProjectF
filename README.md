# WTE · Windows Theme Engine

WTE is a portable Rust app for Windows 11. Apply one palette across your wallpaper, Windows accent, VS Code, Neovim and GlazeWM borders. Start from one of three bundled presets or let WTE pick an accent and build a theme from a photograph. VS Code and Neovim keep ExoVoid syntax colors as you switch wallpapers.

[Run the portable app](#run-the-portable-app) · [Build from source](#build-from-source) · [Apply a preset](#apply-a-preset) · [Create a custom theme](#create-a-custom-theme)

![WTE’s theme library with the Canopy forest wallpaper selected and the Sienna and Blue Hour presets beside it](docs/screenshots/theme-library.png)

## Run the portable app

Extract `WTE-0.2.0-windows-x64.zip` and double-click `wte.exe`. WTE opens in **Library**. VS Code, Neovim and GlazeWM are optional. The apply results identify any enabled integration WTE could not find.

## Build from source

### On Windows

1. Install Microsoft’s C++ Build Tools. In Visual Studio Installer, select **Desktop development with C++** and include the MSVC x64 build tools and a Windows SDK. [Microsoft installation guide](https://learn.microsoft.com/en-us/cpp/build/vscpp-step-0-installation?view=msvc-170)
2. Install stable Rust using the **x64** Windows installer and choose the default MSVC toolchain. [Rust installer](https://rust-lang.org/tools/install/)
3. Open a new PowerShell window in the source directory containing `Cargo.toml`. Build and launch the release executable with the following commands.

```powershell
cargo build --release --locked
.\target\release\wte.exe
```

### From Linux for Windows x64

Install the MinGW-w64 x86-64 toolchain and make its `x86_64-w64-mingw32-gcc`, `x86_64-w64-mingw32-windres` and `x86_64-w64-mingw32-ar` commands available on `PATH`. From the repository root, run the following commands.

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --locked --target x86_64-pc-windows-gnu
```

Copy `target/x86_64-pc-windows-gnu/release/wte.exe` to the Windows machine. The executable includes the preset wallpapers and fonts.

### Run the tests

Run `cargo test --locked` from the repository root. With `nvim` on `PATH`, `cargo test --locked -- --include-ignored` also checks startup and focus-triggered colorscheme changes in a temporary Neovim profile.

## Apply a preset

| Preset | Wallpaper | Palette |
| --- | --- | --- |
| Canopy | Mist over a pine forest in Falcade, Italy | Pine, sage and warm ivory |
| Sienna | A red dune in Deadvlei, Namibia | Clay, copper and sand |
| Blue Hour | The Dolomites above St. Magdalena at dusk | Midnight blue, periwinkle and ice |

1. In **Library**, select a card in **Collection**. Switch between **Wallpaper** and **Editor** in the preview to see the photograph and a code sample in the selected palette.
2. Select **Apply theme**. Windows may open Personalization to finish the desktop change.
3. Read **Apply results** for each enabled component. Restart open Neovim sessions after the first application. If GlazeWM reports that it saved the colors and requests a reload, use your GlazeWM reload binding or restart it.

## Create a custom theme

1. In **Library**, choose **Theme from wallpaper…** and pick a local JPEG or PNG up to 40 megapixels. WTE opens an editable draft named after the file. It samples visible pixels locally, groups similar hues, and selects a prominent accent and a supporting color. Grayscale photos produce neutral palettes. WTE lifts darker accents for contrast on the generated editor surfaces.
2. To start from a preset instead, select its card and choose **New theme**. In the editor, **Choose local image…** replaces the wallpaper. **Use wallpaper colors** regenerates the six swatches from the selected image, including bundled wallpapers. You can tweak each swatch and rename the draft before saving.
3. Adjust **Blur**, **Raindrops** and **Film grain** while watching the preview. The *Alpine Rain* example uses the Blue Hour photograph with blur at **8%**, raindrops at **74%** and film grain at **6%**. The effects render into a still wallpaper, so you can close WTE after applying it.
4. **Accent** supplies the Windows accent and focused GlazeWM border. **Surface** supplies the unfocused border. Clear **Use palette** beside either border to choose its color independently.
5. Under **Components**, select the applications to update. Leave the editor theme fields blank for ExoVoid syntax with wallpaper-based backgrounds, highlights and borders. Enter an installed VS Code theme or Neovim colorscheme name to use that editor's own theme.

![Alpine Rain in the theme editor with six colors extracted from the mountain photograph, wallpaper effects and component choices](docs/screenshots/theme-editor.png)

Choose **Save theme** to copy the imported wallpaper to the local library and return to the collection. Under **Collection**, select **Custom** to find it again. The **Editor** preview shows how the colors read together. Select **Apply theme** when you want to update your setup.

![The saved Alpine Rain theme in the Custom collection, with a code sample over the filtered wallpaper and the Apply theme button](docs/screenshots/saved-theme.png)

## What WTE changes

| Component | Interface and scope |
| --- | --- |
| Windows desktop | WTE opens a local `.theme` file through the registered Windows shell handler. The file specifies a fill-mode wallpaper and an explicit accent color. Personalization and organizational policy control the final desktop appearance. [Microsoft theme file format](https://learn.microsoft.com/en-us/windows/win32/controls/themesfileformat-overview) |
| VS Code | WTE edits the default stable profile at `%APPDATA%\Code\User\settings.json`. Its generated mode selects **ExoVoid Classic Syntax - Dark Soft Blue** and scopes interface overrides to that theme. TextMate rules, semantic token rules, font styles and workbench text colors come from ExoVoid. WTE preserves comments, unrelated settings and other theme scopes. Named profiles, workspace settings and automatic theme selection can take precedence. [VS Code theme settings](https://code.visualstudio.com/docs/configure/themes) |
| Neovim | WTE writes a colorscheme, selection file and Vimscript loader under `stdpath('data')/site/pack/wte/start/wte`. Classic highlights, Tree-sitter captures and LSP token groups use colors from the same ExoVoid theme. After the initial restart, changes load on startup or when the frontend delivers `FocusGained`. [Neovim packages](https://neovim.io/doc/user/pack/) |
| GlazeWM v3 | WTE updates the enabled flag and color for focused and unfocused borders in `%USERPROFILE%\.glzr\glazewm\config.yaml` or the absolute path in `GLAZEWM_CONFIG_PATH`. It requests a reload through the CLI for a standard Program Files installation. Border effects require Windows 11. [GlazeWM configuration](https://github.com/glzr-io/glazewm#config-window-effects) |

### First use with VS Code

WTE reuses an installed ExoVoid extension that provides Dark Soft Blue. Otherwise, on **Apply theme**, it passes the bundled ExoVoid 1.4.0 VSIX to VS Code’s extension manager. The package contains declarative themes and works offline. If WTE cannot find the CLI, **Apply results** shows the local file to select with **Extensions: Install from VSIX…**. Organization extension policies still apply. A named editor override skips this installation step.

## Backups and recovery

WTE stores its library, imported photos, rendered wallpapers and desktop themes in `%LOCALAPPDATA%\WTE`. Before changing an existing VS Code or GlazeWM configuration, it saves the original bytes in `backups\<apply-id>\` under that folder. **Apply results** shows the backup path. WTE replaces each file atomically and reports each integration independently.

- **Windows** — Select your previous theme in Windows Personalization before deleting any WTE wallpaper files.
- **VS Code and GlazeWM** — Restore the relevant `vscode-settings.json` or `glazewm-config.yaml` backup to its original configuration path, then reload the application. If you edited the file after applying the theme, merge the settings you want to keep.
- **Neovim** — Remove `site/pack/wte/start/wte` under `stdpath('data')` and restart Neovim. Run `:echo stdpath('data')` in Neovim to locate that directory.

WTE runs with the current user’s privileges. It works offline without a service, scheduled task, startup entry, direct registry edits or editor extension downloads. The portable build is unsigned. An organization’s application-control policy may require an approved or signed build. The repository includes the Rust source, dependency lockfile and Windows privilege manifest for review.

## Credits

ExoVoid is by GinoPioLoco and uses the MIT license. WTE bundles the published 1.4.0 extension. Its Dark Soft Blue JSON matches the pinned source. [Theme source](https://github.com/gpioloco/ExoVoid/blob/58413770f8575c8835073f6d769c9951f46bbe55/themes/ExoVoidClassicSyntax_DarkSoftBlue.json) · [Extension listing](https://marketplace.visualstudio.com/items?itemName=GinoPioLoco.exovoid) · [ExoVoid license](assets/exovoid/LICENSE)

The bundled photographs use the Unsplash License. The UI uses DM Sans under the SIL Open Font License. [Wallpaper credits](assets/wallpapers/CREDITS.md) · [Unsplash License](https://unsplash.com/license) · [DM Sans license](assets/fonts/OFL.txt)
