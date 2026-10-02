use anyhow::{anyhow, Result};
use macroquad::color::Color;
use xcsim_core::custom_style::{apply as apply_style, clear as clear_style, CustomStyle, ElementStyle, StyleFont};
use std::collections::HashMap;
use std::path::PathBuf;

pub const STYLE_FILE_NAME: &str = "custom_style.xml";

pub fn style_path() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join(STYLE_FILE_NAME))
}

pub fn xml_exists_on_disk() -> bool {
    style_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn read_xml_from_disk() -> Result<Option<String>> {
    let path = style_path()?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(&path)?))
}

pub fn write_xml_to_disk(text: &str) -> Result<()> {
    let path = style_path()?;
    std::fs::write(&path, text)?;
    Ok(())
}

pub fn delete_xml_on_disk() -> Result<()> {
    let path = style_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn load_and_apply_from_disk() -> Result<bool> {
    match read_xml_from_disk()? {
        Some(text) => {
            let style = parse_xml(&text)?;
            apply_style(style);
            Ok(true)
        }
        None => {
            clear_style();
            Ok(false)
        }
    }
}

pub fn disable_active_style() {
    clear_style();
}

pub fn parse_xml(text: &str) -> Result<CustomStyle> {
    let mut style = CustomStyle::default();
    style.enabled = true;

    let elem_re = regex::Regex::new(r"<\s*element\b([^/>]*)/?\s*>")
        .map_err(|e| anyhow!("regex compile: {}", e))?;
    let attr_re = regex::Regex::new(r#"(\w+)\s*=\s*"([^"]*)""#)
        .map_err(|e| anyhow!("regex compile: {}", e))?;

    let mut found_any = false;
    for cap in elem_re.captures_iter(text) {
        found_any = true;
        let attrs_str = &cap[1];
        let mut attrs: HashMap<String, String> = HashMap::new();
        for ac in attr_re.captures_iter(attrs_str) {
            attrs.insert(ac[1].to_ascii_lowercase(), ac[2].to_string());
        }
        let name = match attrs.get("name") {
            Some(n) => n.to_ascii_lowercase(),
            None => continue,
        };
        let elem = build_element_style(&attrs);
        match name.as_str() {
            "score" => style.score = elem,
            "combo_number" | "combonumber" | "combo-number" => style.combo_number = elem,
            "combo" | "combo_text" | "combolabel" => style.combo = elem,
            "accuracy" | "acc" => style.accuracy = elem,
            "pause" => style.pause = elem,
            "bar" | "progress" | "progressbar" => style.bar = elem,
            "name" | "song" | "title" => style.name = elem,
            "level" | "difficulty" | "diff" => style.level = elem,
            "watermark" | "wm" => style.watermark = elem,
            _ => {}
        }
    }

    if !found_any {
        return Err(anyhow!("no <element ... /> tags found"));
    }
    Ok(style)
}

fn build_element_style(attrs: &HashMap<String, String>) -> ElementStyle {
    let mut s = ElementStyle::default();
    if let Some(v) = attrs.get("x") {
        if let Ok(n) = v.parse() {
            s.x = Some(n);
        }
    }
    if let Some(v) = attrs.get("y") {
        if let Ok(n) = v.parse() {
            s.y = Some(n);
        }
    }
    if let Some(v) = attrs.get("anchor_x").or_else(|| attrs.get("anchorx")).or_else(|| attrs.get("ax")) {
        if let Ok(n) = v.parse() {
            s.anchor_x = Some(n);
        }
    }
    if let Some(v) = attrs.get("anchor_y").or_else(|| attrs.get("anchory")).or_else(|| attrs.get("ay")) {
        if let Ok(n) = v.parse() {
            s.anchor_y = Some(n);
        }
    }
    if let Some(v) = attrs.get("size").or_else(|| attrs.get("scale")) {
        if let Ok(n) = v.parse() {
            s.size = Some(n);
        }
    }
    if let Some(v) = attrs.get("color").or_else(|| attrs.get("colour")) {
        s.color = parse_color(v);
    }
    if let Some(v) = attrs.get("font") {
        s.font = match v.to_ascii_lowercase().as_str() {
            "pgr" | "digital" | "scoreboard" => Some(StyleFont::Pgr),
            "default" | "regular" | "body" => Some(StyleFont::Default),
            _ => None,
        };
    }
    if let Some(v) = attrs.get("visible").or_else(|| attrs.get("show")) {
        s.visible = parse_bool(v);
    } else if let Some(v) = attrs.get("hidden").or_else(|| attrs.get("hide")) {
        s.visible = parse_bool(v).map(|b| !b);
    }
    s
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "y" => Some(true),
        "false" | "0" | "no" | "off" | "n" => Some(false),
        _ => None,
    }
}

fn parse_color(s: &str) -> Option<Color> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() != 6 && hex.len() != 8 {
            return None;
        }
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        match bytes.len() {
            3 => Some(Color::new(
                bytes[0] as f32 / 255.0,
                bytes[1] as f32 / 255.0,
                bytes[2] as f32 / 255.0,
                1.0,
            )),
            4 => Some(Color::new(
                bytes[0] as f32 / 255.0,
                bytes[1] as f32 / 255.0,
                bytes[2] as f32 / 255.0,
                bytes[3] as f32 / 255.0,
            )),
            _ => None,
        }
    } else if let Some(inner) = t.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if parts.len() == 4 {
            let r = if parts[0] > 1.0 { parts[0] / 255.0 } else { parts[0] };
            let g = if parts[1] > 1.0 { parts[1] / 255.0 } else { parts[1] };
            let b = if parts[2] > 1.0 { parts[2] / 255.0 } else { parts[2] };
            Some(Color::new(r, g, b, parts[3].clamp(0.0, 1.0)))
        } else {
            None
        }
    } else if let Some(inner) = t.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if parts.len() == 3 {
            let r = if parts[0] > 1.0 { parts[0] / 255.0 } else { parts[0] };
            let g = if parts[1] > 1.0 { parts[1] / 255.0 } else { parts[1] };
            let b = if parts[2] > 1.0 { parts[2] / 255.0 } else { parts[2] };
            Some(Color::new(r, g, b, 1.0))
        } else {
            None
        }
    } else {
        None
    }
}
