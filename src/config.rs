use crate::theme::{Theme, hex};
use anyhow::{Context, Result, ensure};
use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstObject, CstRootNode},
};
use std::str::FromStr;
use yaml_edit::{Mapping, YamlFile};

fn unique(obj: &CstObject, key: &str) -> Result<()> {
    ensure!(
        obj.properties()
            .iter()
            .filter(|p| p.decoded_name().as_deref() == Some(key))
            .count()
            <= 1,
        "Duplicate VS Code setting '{key}'"
    );
    Ok(())
}
fn object(obj: &CstObject, key: &str) -> Result<CstObject> {
    unique(obj, key)?;
    obj.object_value_or_create(key)
        .with_context(|| format!("Expected an object for '{key}'"))
}
fn set(obj: &CstObject, key: &str, value: impl Into<CstInputValue>) -> Result<()> {
    unique(obj, key)?;
    let value = value.into();
    if let Some(p) = obj.get(key) {
        p.set_value(value);
    } else {
        obj.append(key, value);
    }
    Ok(())
}

pub fn vscode(input: &str, t: &Theme) -> Result<String> {
    let options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    };
    let (bom, input) = input
        .strip_prefix('\u{feff}')
        .map_or(("", input), |s| ("\u{feff}", s));
    let root = CstRootNode::parse(input, &options)?;
    let obj = root
        .object_value_or_create()
        .context("VS Code settings must be a JSON object")?;
    if t.code_theme.trim().is_empty() {
        const BASE: &str = "Monokai";
        let scope = format!("[{BASE}]");
        set(&obj, "workbench.colorTheme", BASE)?;
        let workbench = object(&object(&obj, "workbench.colorCustomizations")?, &scope)?;
        let p = &t.palette;
        for (color, keys) in [
            (
                p.background,
                &[
                    "editor.background",
                    "terminal.background",
                    "panel.background",
                    "editorGroupHeader.tabsBackground",
                ][..],
            ),
            (
                p.surface,
                &[
                    "sideBar.background",
                    "activityBar.background",
                    "titleBar.activeBackground",
                    "statusBar.background",
                    "tab.inactiveBackground",
                    "input.background",
                    "dropdown.background",
                    "editorWidget.background",
                    "editor.lineHighlightBackground",
                ][..],
            ),
            (
                p.text,
                &[
                    "foreground",
                    "editor.foreground",
                    "terminal.foreground",
                    "sideBar.foreground",
                    "titleBar.activeForeground",
                    "statusBar.foreground",
                    "tab.activeForeground",
                    "input.foreground",
                ][..],
            ),
            (
                p.muted,
                &[
                    "editorLineNumber.foreground",
                    "descriptionForeground",
                    "tab.inactiveForeground",
                ][..],
            ),
            (
                p.accent,
                &[
                    "focusBorder",
                    "button.background",
                    "activityBar.activeBorder",
                    "tab.activeBorderTop",
                    "editorCursor.foreground",
                    "progressBar.background",
                    "activityBarBadge.background",
                    "textLink.foreground",
                    "list.highlightForeground",
                ][..],
            ),
            (
                p.background,
                &[
                    "button.foreground",
                    "activityBarBadge.foreground",
                    "tab.activeBackground",
                ][..],
            ),
            (
                p.secondary,
                &["terminal.ansiYellow", "editorLineNumber.activeForeground"][..],
            ),
        ] {
            for key in keys {
                set(&workbench, key, hex(color))?;
            }
        }
        set(
            &workbench,
            "editor.selectionBackground",
            format!("{}45", hex(p.accent)),
        )?;
        let tokens = object(&object(&obj, "editor.tokenColorCustomizations")?, &scope)?;
        for (key, rgb) in [
            ("comments", p.muted),
            ("strings", p.secondary),
            ("numbers", p.secondary),
            ("keywords", p.accent),
            ("functions", p.text),
            ("types", p.accent),
            ("variables", p.text),
        ] {
            set(&tokens, key, hex(rgb))?;
        }
        let sem = object(
            &object(&obj, "editor.semanticTokenColorCustomizations")?,
            &scope,
        )?;
        let rules = object(&sem, "rules")?;
        for (key, rgb) in [
            ("class", p.accent),
            ("type", p.accent),
            ("function", p.text),
            ("method", p.text),
            ("parameter", p.secondary),
        ] {
            // Preserve existing font style fields on object-valued semantic rules.
            if let Some(rule) = rules.object_value(key) {
                set(&rule, "foreground", hex(rgb))?;
            } else {
                set(&rules, key, hex(rgb))?;
            }
        }
    } else {
        set(&obj, "workbench.colorTheme", t.code_theme.trim())?;
    }
    Ok(format!("{bom}{root}"))
}

fn submap(parent: &Mapping, key: &str) -> Result<Mapping> {
    if parent.get(key).is_none() {
        parent.set(key, Mapping::new());
    }
    parent
        .get_mapping(key)
        .with_context(|| format!("Expected a mapping at '{key}'"))
}
pub fn glazewm(input: &str, t: &Theme) -> Result<String> {
    // Cross-check the lossless edit against the intended semantic edit before writing a user's YAML.
    let mut expected: serde_yaml_ng::Value = serde_yaml_ng::from_str(input)?;
    ensure!(
        expected.is_mapping(),
        "GlazeWM config must be a YAML mapping"
    );
    let file = YamlFile::from_str(input)?;
    let doc = file.document().context("Empty GlazeWM config")?;
    let root = doc.as_mapping().context("Invalid GlazeWM config")?;
    let effects = submap(&root, "window_effects")?;
    for (key, color) in [
        ("focused_window", t.focused.unwrap_or(t.palette.accent)),
        ("other_windows", t.unfocused.unwrap_or(t.palette.surface)),
    ] {
        let window = submap(&effects, key)?;
        let border = submap(&window, "border")?;
        border.set("enabled", true);
        border.set("color", hex(color).as_str());
        let expected_border = &mut expected["window_effects"][key]["border"];
        expected_border["enabled"] = true.into();
        expected_border["color"] = hex(color).into();
    }
    let output = file.to_string();
    ensure!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&output)? == expected,
        "Could not preserve the structure of this GlazeWM config."
    );
    Ok(output)
}

pub const NVIM_PLUGIN: &str = r#"" Windows Theme Engine
let s:selection = expand('<sfile>:p:h:h') . '/active.txt'
let s:last = []
function! s:WteRefresh() abort
  if !filereadable(s:selection) | return | endif
  let l:active = readfile(s:selection, '', 2)
  if len(l:active) != 2 || l:active ==# s:last || l:active[0] !~# '^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$' | return | endif
  try
    execute 'colorscheme ' . l:active[0]
    let s:last = l:active
  catch /^Vim\%((\a\+)\)\=:E185/
    echohl WarningMsg | echom 'WTE: colorscheme is unavailable: ' . l:active[0] | echohl None
  endtry
endfunction
augroup wte_theme_engine
  autocmd!
  autocmd VimEnter,FocusGained * call <SID>WteRefresh()
augroup END
"#;

pub fn nvim_palette(t: &Theme) -> String {
    let p = &t.palette;
    let (bg, surface, fg, muted, accent, second) = (
        hex(p.background),
        hex(p.surface),
        hex(p.text),
        hex(p.muted),
        hex(p.accent),
        hex(p.secondary),
    );
    let mut out = format!(
        "\" Windows Theme Engine\nset termguicolors\nset background={}\nhighlight clear\nif exists('syntax_on') | syntax reset | endif\nlet g:colors_name = 'wte-palette'\n",
        if (p.background[0] as u32 * 299
            + p.background[1] as u32 * 587
            + p.background[2] as u32 * 114)
            > 145000
        {
            "light"
        } else {
            "dark"
        }
    );
    for (names, fore, back) in [
        ("Normal NormalNC EndOfBuffer", &fg, &bg),
        ("NormalFloat Pmenu StatusLineNC", &fg, &surface),
        ("LineNr Comment NonText", &muted, &bg),
        (
            "CursorLineNr Title Directory Special Statement Keyword Type PreProc",
            &accent,
            &bg,
        ),
        ("Cursor", &bg, &accent),
        ("Visual PmenuSel StatusLine Search", &bg, &accent),
        ("ColorColumn CursorLine", &fg, &surface),
        ("FloatBorder WinSeparator", &accent, &surface),
        (
            "String Constant Number Boolean Character Identifier",
            &second,
            &bg,
        ),
        ("Function", &fg, &bg),
    ] {
        for name in names.split_whitespace() {
            out += &format!("highlight {name} guifg={fore} guibg={back}\n");
        }
    }
    out.push_str("highlight! link @keyword Keyword\nhighlight! link @function Function\nhighlight! link @string String\nhighlight! link @type Type\nhighlight! link @comment Comment\n");
    out
}

pub fn desktop_theme(t: &Theme, wallpaper: &str) -> Result<Vec<u8>> {
    ensure!(
        !wallpaper.contains(['\r', '\n', '\0']),
        "Invalid wallpaper path."
    );
    let color = hex(t.palette.accent);
    let content = format!(
        "[Theme]\r\nDisplayName=WTE {}\r\n\r\n[Control Panel\\Desktop]\r\nWallpaper={}\r\nTileWallpaper=0\r\nWallpaperStyle=10\r\n\r\n[VisualStyles]\r\nPath=%SystemRoot%\\resources\\Themes\\Aero\\Aero.msstyles\r\nColorStyle=NormalColor\r\nSize=NormalSize\r\nAutoColorization=0\r\nColorizationColor=0xFF{}\r\n\r\n[MasterThemeSelector]\r\nMTSM=DABJDKT\r\n",
        t.name,
        wallpaper,
        &color[1..]
    );
    Ok([0xfeffu16]
        .into_iter()
        .chain(content.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect())
}
