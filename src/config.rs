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
        set(&obj, "workbench.colorTheme", exovoid::NAME)?;
        let scope = format!("[{}]", exovoid::NAME);
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
    let bg = hex(p.background);
    let surface = hex(p.surface);
    let accent = hex(p.accent);
    let selection = hex(mix(p.surface, p.accent, 0.35));
    let fg = exovoid::workbench("editor.foreground");
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
    for (groups, fore, back) in [
        ("Normal NormalNC EndOfBuffer", fg, bg.as_str()),
        (
            "NormalFloat Pmenu StatusLineNC StatusLine",
            fg,
            surface.as_str(),
        ),
        (
            "LineNr",
            exovoid::workbench("editorLineNumber.foreground"),
            bg.as_str(),
        ),
        (
            "CursorLineNr",
            exovoid::workbench("editorLineNumber.activeForeground"),
            surface.as_str(),
        ),
        (
            "FloatBorder WinSeparator",
            accent.as_str(),
            surface.as_str(),
        ),
        ("Cursor", bg.as_str(), accent.as_str()),
        ("PmenuSel Search", fg, selection.as_str()),
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
        let fore = exovoid::syntax(scope);
        for group in groups.split_whitespace() {
            out += &format!("highlight {group} guifg={fore} guibg=NONE gui=NONE\n");
        }
    }
    for (token, value) in exovoid::SOURCE["semanticTokenColors"].as_object().unwrap() {
        let fore = value.as_str().expect("bundled semantic token color");
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
