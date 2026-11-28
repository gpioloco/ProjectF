use crate::{
    exovoid,
    palette::mix,
    theme::{Palette, Theme, hex},
};
use anyhow::{Context, Result, ensure};
use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstObject, CstRootNode},
};
use std::str::FromStr;
use yaml_edit::{Mapping, YamlFile};

pub(crate) fn unique(obj: &CstObject, key: &str) -> Result<()> {
    ensure!(
        obj.properties()
            .iter()
            .filter(|p| p.decoded_name().as_deref() == Some(key))
            .count()
            <= 1,
        "Duplicate setting '{key}'"
    );
    Ok(())
}
pub(crate) fn object(obj: &CstObject, key: &str) -> Result<CstObject> {
    unique(obj, key)?;
    obj.object_value_or_create(key)
        .with_context(|| format!("Expected an object for '{key}'"))
}
pub(crate) fn set(obj: &CstObject, key: &str, value: impl Into<CstInputValue>) -> Result<()> {
    unique(obj, key)?;
    let value = value.into();
    if let Some(p) = obj.get(key) {
        p.set_value(value);
    } else {
        obj.append(key, value);
    }
    Ok(())
}

pub(crate) fn jsonc(input: &str) -> Result<(&str, CstRootNode)> {
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
    Ok((bom, CstRootNode::parse(input, &options)?))
}

pub fn vscode(input: &str, t: &Theme) -> Result<String> {
    let (bom, root) = jsonc(input)?;
    let obj = root
        .object_value_or_create()
        .context("VS Code settings must be a JSON object")?;
    if t.code_theme.trim().is_empty() {
        let name = exovoid::name(t.mode);
        set(&obj, "workbench.colorTheme", name)?;
        let scope = format!("[{name}]");
        let workbench = object(&object(&obj, "workbench.colorCustomizations")?, &scope)?;
        for (key, value) in exovoid::SOURCE["colors"].as_object().unwrap() {
            if let Some(color) = workbench_override(key, value.as_str().unwrap(), &t.palette) {
                set(&workbench, key, color)?;
            }
        }
        // The upstream theme inherits the terminal surface and active activity-bar indicator.
        set(&workbench, "terminal.background", hex(t.palette.background))?;
        set(
            &workbench,
            "activityBar.activeBorder",
            hex(t.palette.accent),
        )?;
    } else {
        set(&obj, "workbench.colorTheme", t.code_theme.trim())?;
    }
    Ok(format!("{bom}{root}"))
}

/// Recolor the workbench, keeping ExoVoid's text, ANSI colors and overlay opacity intact.
fn workbench_override(key: &str, original: &str, p: &Palette) -> Option<String> {
    let id = key.to_ascii_lowercase();
    if id.contains("foreground") || id.starts_with("terminal.ansi") {
        return None;
    }
    let rgb = if matches!(
        key,
        "editor.background"
            | "editorGutter.background"
            | "tab.activeBackground"
            | "minimap.background"
            | "editorGroup.emptyBackground"
            | "editorCursor.background"
    ) {
        p.background
    } else if [
        "error",
        "warning",
        "added",
        "modified",
        "deleted",
        "inserted",
        "removed",
        "conflicting",
        "nomatches",
    ]
    .iter()
    .any(|word| id.contains(word))
    {
        // Retain recognizable diagnostic and diff hues on the new surfaces.
        mix(p.surface, exovoid::rgb(original), 0.68)
    } else if id.contains("shadow") {
        mix(p.background, [0; 3], 0.65)
    } else if key == "focusBorder" || id.contains("activeborder") || id == "progressbar.background"
    {
        p.accent
    } else if id.contains("guide") || id.starts_with("scrollbarslider.") {
        mix(p.surface, p.muted, 0.45)
    } else if id.contains("border") || id.contains("outline") || id.contains("stroke") {
        mix(p.surface, p.accent, 0.44)
    } else if id.contains("match") || id.contains("highlight") || id.contains("incoming") {
        mix(p.surface, p.secondary, 0.40)
    } else if id.contains("selection")
        || id.contains("selected")
        || id.contains("hover")
        || id.contains("activebackground")
        || id.contains("badge")
        || id.starts_with("button.")
        || id.contains("dropbackground")
    {
        mix(p.surface, p.accent, 0.35)
    } else {
        p.surface
    };
    Some(format!("{}{}", hex(rgb), &original[7..]))
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
    window_manager(input, t, false)
}

pub fn rillshift(input: &str, t: &Theme) -> Result<String> {
    window_manager(input, t, true)
}

fn window_manager(input: &str, t: &Theme, include_bar_accent: bool) -> Result<String> {
    // Cross-check the lossless edit against the intended semantic edit before writing a user's YAML.
    let mut expected: serde_yaml_ng::Value = serde_yaml_ng::from_str(input)?;
    ensure!(
        expected.is_mapping(),
        "Window manager config must be a YAML mapping"
    );
    let file = YamlFile::from_str(input)?;
    let doc = file.document().context("Empty window manager config")?;
    let root = doc.as_mapping().context("Invalid window manager config")?;
    let effects = submap(&root, "window_effects")?;
    for (key, color) in [
        ("focused_window", t.focused.unwrap_or(t.palette.accent)),
        ("other_windows", t.unfocused.unwrap_or(t.palette.surface)),
    ] {
        let window = submap(&effects, key)?;
        let border = submap(&window, "border")?;
        border.set("color", hex(color).as_str());
        let expected_border = &mut expected["window_effects"][key]["border"];
        expected_border["color"] = hex(color).into();
    }
    if include_bar_accent {
        // Rillshift accepts the legacy section as a serde alias. Keep its spelling.
        let legacy = root.get("projectwm").is_some();
        ensure!(
            !(legacy && root.get("rillshift").is_some()),
            "Both rillshift and projectwm sections exist"
        );
        let section = if legacy { "projectwm" } else { "rillshift" };
        let bar = submap(&submap(&root, section)?, "bar")?;
        let accent = t.palette.accent;
        if let Some(seq) = bar.get_sequence("accent_color") {
            ensure!(seq.len() == 3, "Expected three accent_color channels");
            for (i, channel) in accent.into_iter().enumerate() {
                seq.set(i, channel);
            }
        } else {
            let seq = yaml_edit::Sequence::new_flow();
            for channel in accent {
                seq.push(channel);
            }
            bar.set("accent_color", seq);
        }
        expected[section]["bar"]["accent_color"] = serde_yaml_ng::to_value(accent)?;
    }
    let output = file.to_string();
    ensure!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&output)? == expected,
        "Could not preserve the structure of this window manager config."
    );
    Ok(output)
}

pub const NVIM_CONFIG_LOADER: &str = r#"-- Window Theme Engine
if vim.g.loaded_wte_theme_engine then return end
local package_dir = vim.fn.stdpath("data") .. "/site/pack/wte/start/wte"
local loader = package_dir .. "/plugin/wte.vim"
if vim.fn.filereadable(loader) == 1 then
  vim.opt.runtimepath:append(package_dir)
  vim.cmd("source " .. vim.fn.fnameescape(loader))
end
"#;

pub const NVIM_PLUGIN: &str = r#"" Window Theme Engine
if exists('g:loaded_wte_theme_engine') | finish | endif
let g:loaded_wte_theme_engine = 1
let s:selection = expand('<sfile>:p:h:h') . '/active.txt'
let s:last = []
function! s:WteRefresh() abort
  if !filereadable(s:selection) | return | endif
  let l:active = readfile(s:selection, '', 2)
  if len(l:active) != 2 || (l:active ==# s:last && get(g:, 'colors_name', '') ==# l:active[0]) || l:active[0] !~# '^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*$' | return | endif
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
    let bg = hex(p.background);
    let surface = hex(p.surface);
    let accent = hex(p.accent);
    let selection = hex(mix(p.surface, p.accent, 0.35));
    let ink = |value| hex(exovoid::ink(t.mode, value));
    let fg = ink(exovoid::workbench("editor.foreground"));
    let line = ink(exovoid::workbench("editorLineNumber.foreground"));
    let active_line = ink(exovoid::workbench("editorLineNumber.activeForeground"));
    let mut out = format!(
        "\" Window Theme Engine\nset termguicolors\nset background={}\nhighlight clear\nif exists('syntax_on') | syntax reset | endif\nlet g:colors_name = 'wte-palette'\n",
        t.mode.label().to_ascii_lowercase()
    );
    for (groups, fore, back) in [
        ("Normal NormalNC EndOfBuffer", fg.as_str(), bg.as_str()),
        (
            "NormalFloat Pmenu StatusLineNC StatusLine",
            fg.as_str(),
            surface.as_str(),
        ),
        ("LineNr", line.as_str(), bg.as_str()),
        ("CursorLineNr", active_line.as_str(), surface.as_str()),
        (
            "FloatBorder WinSeparator",
            accent.as_str(),
            surface.as_str(),
        ),
        ("Cursor", bg.as_str(), accent.as_str()),
        ("PmenuSel Search", fg.as_str(), selection.as_str()),
        ("Visual", "NONE", selection.as_str()),
        ("ColorColumn CursorLine", "NONE", surface.as_str()),
    ] {
        for group in groups.split_whitespace() {
            out += &format!("highlight {group} guifg={fore} guibg={back} gui=NONE\n");
        }
    }
    for (groups, scope) in [
        ("Comment @comment", "comment"),
        ("String Character @string @lsp.type.string", "string"),
        ("SpecialChar @string.escape", "constant.character.escape"),
        ("Number Float @number @lsp.type.number", "constant.numeric"),
        ("Boolean @boolean @variable.builtin", "constant.language"),
        (
            "Constant @constant @lsp.type.enumMember",
            "variable.other.constant",
        ),
        (
            "Identifier @variable @variable.parameter @property @lsp.type.variable @lsp.type.parameter @lsp.type.property",
            "variable",
        ),
        (
            "Function @function @function.call @function.method @function.method.call @lsp.type.function @lsp.type.method",
            "entity.name.function",
        ),
        (
            "Type Typedef Structure @type @lsp.type.type @lsp.type.class @lsp.type.struct @lsp.type.enum @lsp.type.namespace",
            "entity.name.type",
        ),
        (
            "Keyword Statement StorageClass @keyword @keyword.function @type.builtin @lsp.type.keyword",
            "keyword",
        ),
        (
            "Conditional Repeat Exception @keyword.conditional @keyword.repeat @keyword.return @keyword.exception",
            "keyword.control",
        ),
        ("Operator @operator @lsp.type.operator", "keyword.operator"),
        (
            "PreProc Include Define Macro PreCondit",
            "meta.preprocessor",
        ),
        ("Delimiter @punctuation", "meta.template.expression"),
        ("Label", "entity.name.label"),
        ("Tag @tag", "entity.name.tag"),
        ("@tag.attribute", "entity.other.attribute-name"),
        ("@string.regexp", "string.regexp"),
    ] {
        let fore = ink(exovoid::syntax(scope));
        for group in groups.split_whitespace() {
            out += &format!("highlight {group} guifg={fore} guibg=NONE gui=NONE\n");
        }
    }
    for (token, value) in exovoid::SOURCE["semanticTokenColors"].as_object().unwrap() {
        let fore = ink(value.as_str().expect("bundled semantic token color"));
        out += &format!("highlight @lsp.type.{token} guifg={fore} guibg=NONE gui=NONE\n");
    }
    out
}

pub fn desktop_theme(t: &Theme, wallpaper: &str) -> Result<Vec<u8>> {
    ensure!(
        !wallpaper.contains(['\r', '\n', '\0']),
        "Invalid wallpaper path."
    );
    let color = hex(t.palette.accent);
    let content = format!(
        "[Theme]\r\nDisplayName=WTE {}\r\n\r\n[Control Panel\\Desktop]\r\nWallpaper={}\r\nTileWallpaper=0\r\nWallpaperStyle=10\r\n\r\n[VisualStyles]\r\nPath=%SystemRoot%\\resources\\Themes\\Aero\\Aero.msstyles\r\nColorStyle=NormalColor\r\nSize=NormalSize\r\nSystemMode={mode}\r\nAppMode={mode}\r\nAutoColorization=0\r\nColorizationColor=0xFF{}\r\n\r\n[MasterThemeSelector]\r\nMTSM=DABJDKT\r\n",
        t.name,
        wallpaper,
        &color[1..],
        mode = t.mode.label(),
    );
    Ok([0xfeffu16]
        .into_iter()
        .chain(content.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect())
}

/// A separate declarative light base gives VS Code light native widgets and keeps
/// customizations for the original dark theme in their own settings scope.
pub fn light_vsix() -> Result<Vec<u8>> {
    use crate::theme::{Appearance, Photo};
    let mut base = Theme::preset(Photo::Canopy);
    base.set_mode(Appearance::Light);
    let mut theme = exovoid::SOURCE.clone();
    theme["name"] = exovoid::LIGHT_NAME.into();
    theme["type"] = "light".into();
    let ink = |value: &str| hex(exovoid::ink(Appearance::Light, value));
    for (key, value) in theme["colors"].as_object_mut().unwrap() {
        let source = value.as_str().unwrap();
        *value = workbench_override(key, source, &base.palette)
            .unwrap_or_else(|| format!("{}{}", ink(source), &source[7..]))
            .into();
    }
    for rule in theme["tokenColors"].as_array_mut().unwrap() {
        if let Some(value) = rule["settings"]["foreground"].as_str() {
            rule["settings"]["foreground"] = ink(value).into();
        }
    }
    for value in theme["semanticTokenColors"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *value = ink(value.as_str().unwrap()).into();
    }
    crate::archive(&[
        (
            "[Content_Types].xml",
            include_bytes!("../assets/exovoid-light/[Content_Types].xml"),
        ),
        (
            "extension.vsixmanifest",
            include_bytes!("../assets/exovoid-light/extension.vsixmanifest"),
        ),
        (
            "extension/package.json",
            include_bytes!("../assets/exovoid-light/package.json"),
        ),
        (
            "extension/LICENSE.txt",
            include_bytes!("../assets/exovoid/LICENSE"),
        ),
        ("extension/theme.json", &serde_json::to_vec(&theme)?),
    ])
}
