# WTE · Windows Theme Engine

WTE is a portable Rust app for Windows 11. Apply one palette across your wallpaper, Windows appearance, VS Code, Neovim and GlazeWM borders. Export matching Chrome and Firefox themes. Start from one of three bundled presets or let WTE pick an accent and build a theme from a photograph. VS Code and Neovim share ExoVoid syntax colors in both modes, with darker ink for light backgrounds.

[Run the portable app](#run-the-portable-app) · [Build from source](#build-from-source) · [Apply a preset](#apply-a-preset) · [Create a custom theme](#create-a-custom-theme) · [Browser setup](#load-a-browser-theme) · [Screenshots](#screenshot-gallery)

![WTE’s theme library with the Canopy forest wallpaper selected and the Sienna and Blue Hour presets beside it](docs/screenshots/theme-library.png)

## Run the portable app

Extract `WTE-0.3.0-windows-x64.zip` and double-click `wte.exe`. WTE opens in **Library**. Choose **App · Light** or **App · Dark** in the top bar to change the WTE window immediately. It remembers this preference when you reopen it. Each integration is optional. The apply results identify any enabled integration WTE could not find.

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

1. In **Library**, choose **Theme from wallpaper…** and pick a local JPEG or PNG up to 40 megapixels. WTE opens an editable draft named after the file. It samples visible pixels locally, groups similar hues, and selects a prominent accent and a supporting color. Grayscale photos produce neutral palettes. WTE adjusts the extracted colors for contrast against the selected surfaces.
2. To start from a preset instead, select its card and choose **New theme**. In the editor, **Choose local image…** replaces the wallpaper. **Use wallpaper colors** regenerates the six swatches from the selected image, including bundled wallpapers. You can tweak each swatch and rename the draft before saving.
3. In **Color palette → Theme mode**, choose **Light** or **Dark** for this theme. Switching back restores the swatches you edited in that mode. **Use wallpaper colors** rebuilds both palettes from the photo. Themes saved before version 0.3 open in dark mode with their original swatches.
4. Adjust **Blur**, **Raindrops** and **Film grain** while watching the preview. The *Alpine Rain* example uses the Blue Hour photograph with blur at **8%**, raindrops at **74%** and film grain at **6%**. The effects render into a still wallpaper, so you can close WTE after applying it.
5. **Accent** supplies the Windows accent and focused GlazeWM border. **Surface** supplies the unfocused border. Clear **Use palette** beside either border to choose its color independently.
6. Under **Components**, select the applications to update. Leave the editor theme fields blank for ExoVoid syntax with wallpaper-based backgrounds, highlights and borders. Enter an installed VS Code theme or Neovim colorscheme name to use that editor's own theme.

![Alpine Rain in the theme editor with WTE in dark mode, a light code preview, wallpaper effects and both browser components selected](docs/screenshots/theme-editor.png)

Choose **Save theme** to copy the imported wallpaper to the local library and return to the collection. Under **Collection**, select **Custom** to find it again. The **Editor** preview shows how the colors read together. Select **Apply theme** when you want to update your setup.

![The saved Alpine Rain theme in the Custom collection, with a code sample over the filtered wallpaper and the Apply theme button](docs/screenshots/saved-theme.png)

## What WTE changes

| Component | Interface and scope |
| --- | --- |
| Windows desktop | WTE opens a local `.theme` file through the registered Windows shell handler. The file specifies a fill-mode wallpaper, an explicit accent color, and the light or dark appearance for Windows and apps. Personalization and organizational policy control the final desktop appearance. [Microsoft theme file format](https://learn.microsoft.com/en-us/windows/win32/controls/themesfileformat-overview) |
| VS Code | WTE edits the default stable profile at `%APPDATA%\Code\User\settings.json`. Dark themes use **ExoVoid Classic Syntax - Dark Soft Blue**. Light themes use **WTE ExoVoid Light**, which keeps the same TextMate scopes, semantic token rules and font styles, and darkens the foreground colors. WTE scopes the palette overrides to the selected base. WTE preserves comments, unrelated settings and other theme scopes. Named profiles, workspace settings and automatic theme selection can take precedence. [VS Code theme settings](https://code.visualstudio.com/docs/configure/themes) |
| Neovim | WTE writes a colorscheme, selection file and Vimscript loader under `stdpath('data')/site/pack/wte/start/wte`. Classic highlights, Tree-sitter captures and LSP token groups use colors from the same ExoVoid theme. After the initial restart, changes load on startup or when the frontend delivers `FocusGained`. [Neovim packages](https://neovim.io/doc/user/pack/) |
| GlazeWM v3 | WTE updates the enabled flag and color for focused and unfocused borders in `%USERPROFILE%\.glzr\glazewm\config.yaml` or the absolute path in `GLAZEWM_CONFIG_PATH`. It requests a reload through the CLI for a standard Program Files installation. Border effects require Windows 11. [GlazeWM configuration](https://github.com/glzr-io/glazewm#config-window-effects) |
| Chrome | Exports a Manifest V3 theme folder with colors for the frame, toolbar, tabs, address bar and new tab page. [Chrome theme format](https://developer.chrome.com/docs/extensions/develop/ui/themes) |
| Firefox | Exports a static WebExtension theme as a single XPI with colors for the frame, toolbar, tabs, address bar and new tab page. It declares the theme’s light or dark color scheme. [Firefox theme format](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/theme) |

### First use with VS Code

On **Apply theme**, WTE checks for the selected ExoVoid base. If needed, it passes the bundled upstream 1.4.0 VSIX for dark mode, or a locally generated WTE ExoVoid Light VSIX for light mode, to VS Code’s extension manager. Both packages are declarative and work offline. If WTE cannot find the CLI, **Apply results** shows the local file to select with **Extensions: Install from VSIX…**. Organization extension policies apply. A named editor override skips this installation step.

## Load a browser theme

To add a browser to a preset, choose **Edit theme** to make a custom copy. In the editor, enable **Chrome**, **Firefox**, or both under **Components**. Choose **Save theme**, then **Apply theme** in the library. **Apply results → Copy path** gives you the exact folder or file for each browser. Browser components start turned off, including when opening older libraries.

- **Chrome.** Open `chrome://extensions`. If your organization permits it, enable **Developer mode**, choose **Load unpacked** and select the exported **chrome** folder. Check **Settings → Appearance → Theme** for the WTE theme name. [Chrome’s local installation guide](https://developer.chrome.com/docs/extensions/get-started/tutorial/hello-world#load-unpacked)
- **Firefox.** Open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on** and select **firefox.xpi**. Check **Add-ons and themes → Themes** for the enabled theme. A temporary theme lasts until Firefox restarts. Lasting installation in standard Firefox requires a Mozilla-signed package. [Firefox temporary installation](https://extensionworkshop.com/documentation/develop/temporary-installation-in-firefox/) · [Mozilla signing and distribution](https://extensionworkshop.com/documentation/publish/signing-and-distribution-overview/)

WTE keeps each theme’s exports under `%LOCALAPPDATA%\WTE\browsers\wte-<theme-id>\`. After changing a theme, apply it in WTE and load its updated export in the browser. Each browser controls activation. Chrome uses its own appearance setting for internal pages such as Settings. The exports contain a color manifest and request no permissions.

## Backups and recovery

WTE stores its library, imported photos, rendered wallpapers and desktop themes in `%LOCALAPPDATA%\WTE`. Before changing an existing VS Code or GlazeWM configuration, it saves the original bytes in `backups\<apply-id>\` under that folder. **Apply results** shows the backup path. WTE replaces each file atomically and reports each integration independently.

- **Windows** — Select your previous theme in Windows Personalization before deleting any WTE wallpaper files.
- **VS Code and GlazeWM** — Restore the relevant `vscode-settings.json` or `glazewm-config.yaml` backup to its original configuration path, then reload the application. If you edited the file after applying the theme, merge the settings you want to keep.
- **Browsers** — Restore the default theme in Chrome’s Appearance settings, or enable a different theme in Firefox’s Add-ons Manager.
- **Neovim** — Remove `site/pack/wte/start/wte` under `stdpath('data')` and restart Neovim. Run `:echo stdpath('data')` in Neovim to locate that directory.

WTE runs with the current user’s privileges. It works offline without a service, scheduled task, startup entry, direct registry edits or editor extension downloads. The portable build is unsigned. An organization’s application-control policy may require an approved or signed build. The repository includes the Rust source, dependency lockfile and Windows privilege manifest for review.

## Credits

ExoVoid is by GinoPioLoco and uses the MIT license. WTE bundles the published 1.4.0 extension. Its Dark Soft Blue JSON matches the pinned source. [Theme source](https://github.com/gpioloco/ExoVoid/blob/58413770f8575c8835073f6d769c9951f46bbe55/themes/ExoVoidClassicSyntax_DarkSoftBlue.json) · [Extension listing](https://marketplace.visualstudio.com/items?itemName=GinoPioLoco.exovoid) · [ExoVoid license](assets/exovoid/LICENSE)

The bundled photographs use the Unsplash License. The UI uses DM Sans under the SIL Open Font License. [Wallpaper credits](assets/wallpapers/CREDITS.md) · [Unsplash License](https://unsplash.com/license) · [DM Sans license](assets/fonts/OFL.txt)

## Screenshot gallery

The WTE screenshots show the Windows build running under Wine. The component screenshots show the generated themes loaded in isolated Linux application profiles. Select a thumbnail to open it at full size.

| Sienna preset | Blue Hour preset |
| --- | --- |
| [![Sienna with its copper and sand swatches and red dune wallpaper](docs/screenshots/preset-sienna.png)](docs/screenshots/preset-sienna.png) | [![Blue Hour with its mountain wallpaper and blue and periwinkle swatches](docs/screenshots/preset-blue-hour.png)](docs/screenshots/preset-blue-hour.png) |

| VS Code · Alpine Rain, light | Neovim · Sienna Dusk, dark |
| --- | --- |
| [![Rust in VS Code with pale Alpine Rain surfaces and darker ExoVoid syntax](docs/screenshots/vscode-light.png)](docs/screenshots/vscode-light.png) | [![Rust in Neovim with Sienna’s dark brown background and original ExoVoid syntax](docs/screenshots/neovim-dark.png)](docs/screenshots/neovim-dark.png) |

| Google Chrome for Testing · Sienna Dusk | Firefox · Alpine Rain |
| --- | --- |
| [![Chrome Appearance settings with WTE Sienna Dusk active and a copper and brown toolbar](docs/screenshots/chrome-dark.png)](docs/screenshots/chrome-dark.png) | [![Firefox’s Themes screen with WTE Alpine Rain enabled and a pale blue-gray toolbar](docs/screenshots/firefox-light.png)](docs/screenshots/firefox-light.png) |

The Alpine Rain desktop theme points Windows to this rendered wallpaper, shown here at reduced resolution.

![Blue Hour mountain wallpaper after applying blur, raindrops and film grain](docs/screenshots/alpine-rain-wallpaper.jpg)
