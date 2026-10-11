//! Versioned, portable presentation document. The same validator runs in the host and WASM guest.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MAX_DOCUMENT_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub id: String,
    pub revision: u64,
    pub title: String,
    #[serde(default)]
    pub brief: String,
    pub theme: Theme,
    pub slides: Vec<Slide>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub script: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub muted: String,
    pub font: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#101b2d".into(),
            foreground: "#f6f3eb".into(),
            accent: "#baf66d".into(),
            muted: "#a5b1c2".into(),
            font: "Arial".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slide {
    pub id: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub transition: String,
    pub elements: Vec<Element>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub style: Value,
    #[serde(default)]
    pub animation: String,
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub data_url: String,
    #[serde(default)]
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub slide_id: String,
    pub element_id: String,
    pub message: String,
}

pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}

impl Document {
    pub fn new(id: String, title: String) -> Self {
        Self {
            schema_version: 1,
            id,
            revision: 0,
            title: title.clone(),
            brief: String::new(),
            theme: Theme::default(),
            assets: vec![],
            css: String::new(),
            script: String::new(),
            slides: vec![Slide {
                id: "slide-1".into(),
                notes: String::new(),
                background: None,
                transition: "fade".into(),
                elements: vec![Element {
                    id: "title-1".into(),
                    kind: "text".into(),
                    x: 90.,
                    y: 220.,
                    w: 1050.,
                    h: 180.,
                    text: title,
                    asset_id: None,
                    style: json!({"fontSize":64,"bold":true}),
                    animation: "fade".into(),
                    delay_ms: 0,
                    data: Value::Null,
                }],
            }],
        }
    }
    pub fn validate(&self) -> Result<Vec<Diagnostic>, String> {
        require(self.schema_version == 1, "unsupported schema_version")?;
        require(valid_id(&self.id), "invalid project id")?;
        require(
            !self.title.trim().is_empty() && self.title.len() <= 300,
            "title must contain 1–300 bytes",
        )?;
        let css: String = self
            .css
            .to_ascii_lowercase()
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        require(
            ![
                "@import",
                "url(",
                "url (",
                "expression",
                "@font-face",
                "<",
                ">",
                "\\",
            ]
            .iter()
            .any(|s| css.contains(s)),
            "CSS must contain local styles only",
        )?;
        require(
            self.brief.len() <= 16000 && self.css.len() <= 64000 && self.script.len() <= 64000,
            "source or brief too large",
        )?;
        require(
            !self.slides.is_empty() && self.slides.len() <= 60,
            "a presentation needs 1–60 slides",
        )?;
        require(self.assets.len() <= 50, "too many assets")?;
        for c in [
            &self.theme.background,
            &self.theme.foreground,
            &self.theme.accent,
            &self.theme.muted,
        ] {
            require(color(c), "theme colors must use #RRGGBB")?;
        }
        require(
            matches!(
                self.theme.font.as_str(),
                "Arial" | "Georgia" | "Verdana" | "Helvetica" | "Courier New"
            ),
            "unsupported font",
        )?;
        let mut asset_ids = BTreeSet::new();
        for a in &self.assets {
            require(
                valid_id(&a.id) && asset_ids.insert(a.id.clone()),
                "invalid or duplicate asset id",
            )?;
            require(
                a.name.len() <= 300 && a.description.len() <= 2000,
                "asset metadata too long",
            )?;
            require(
                a.data_url.len() <= 7 * 1024 * 1024
                    && [
                        "data:image/png;base64,",
                        "data:image/jpeg;base64,",
                        "data:image/webp;base64,",
                    ]
                    .iter()
                    .any(|p| a.data_url.starts_with(p)),
                "asset must be a PNG, JPEG or WebP up to 5 MB",
            )?;
            let data = a.data_url.split_once(',').unwrap().1;
            require(
                !data.is_empty()
                    && data
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+/=\r\n".contains(&b)),
                "invalid image encoding",
            )?;
        }
        let mut slide_ids = BTreeSet::new();
        let mut diagnostics = vec![];
        for s in &self.slides {
            require(
                valid_id(&s.id) && slide_ids.insert(&s.id),
                "invalid or duplicate slide id",
            )?;
            require(
                s.notes.len() <= 16000 && s.elements.len() <= 100,
                "slide too large",
            )?;
            require(
                matches!(s.transition.as_str(), "" | "none" | "fade" | "slide"),
                "unsupported transition",
            )?;
            if let Some(c) = &s.background {
                require(color(c), "invalid slide color")?;
            }
            let mut ids = BTreeSet::new();
            for e in &s.elements {
                require(
                    valid_id(&e.id) && ids.insert(&e.id),
                    "invalid or duplicate element id",
                )?;
                require(
                    matches!(
                        e.kind.as_str(),
                        "text" | "image" | "shape" | "chart" | "table"
                    ),
                    "unsupported element kind",
                )?;
                require(
                    [e.x, e.y, e.w, e.h].iter().all(|n| n.is_finite())
                        && e.x >= 0.
                        && e.y >= 0.
                        && e.w >= 1.
                        && e.h >= 1.
                        && e.w <= 1280.
                        && e.h <= 720.,
                    "invalid element geometry",
                )?;
                require(
                    e.text.len() <= 16000 && e.delay_ms <= 10000,
                    "element content too long",
                )?;
                require(
                    matches!(
                        e.animation.as_str(),
                        "" | "none" | "fade" | "rise" | "slide" | "zoom"
                    ),
                    "unsupported animation",
                )?;
                if e.kind == "image" {
                    require(
                        e.asset_id.as_ref().is_some_and(|id| asset_ids.contains(id)),
                        "image references missing asset",
                    )?;
                }
                require(
                    e.style.is_null() || e.style.is_object(),
                    "style must be an object",
                )?;
                if let Some(style) = e.style.as_object() {
                    for (k, v) in style {
                        match k.as_str() {
                            "fontSize" => require(
                                v.as_f64().is_some_and(|n| (10.0..=160.0).contains(&n)),
                                "fontSize out of range",
                            )?,
                            "color" | "fill" => {
                                require(v.as_str().is_some_and(color), "invalid style color")?
                            }
                            "bold" | "italic" => require(v.is_boolean(), "invalid font style")?,
                            "align" => require(
                                v.as_str()
                                    .is_some_and(|s| matches!(s, "left" | "center" | "right")),
                                "invalid alignment",
                            )?,
                            "radius" => require(
                                v.as_f64().is_some_and(|n| (0.0..=100.0).contains(&n)),
                                "invalid radius",
                            )?,
                            "opacity" => require(
                                v.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)),
                                "invalid opacity",
                            )?,
                            _ => return Err(format!("unsupported style property: {k}")),
                        }
                    }
                }
                if e.kind == "chart" {
                    let labels = e
                        .data
                        .get("labels")
                        .and_then(Value::as_array)
                        .ok_or("chart requires labels")?;
                    let values = e
                        .data
                        .get("values")
                        .and_then(Value::as_array)
                        .ok_or("chart requires values")?;
                    require(
                        !labels.is_empty()
                            && labels.len() <= 20
                            && labels.len() == values.len()
                            && labels
                                .iter()
                                .all(|v| v.as_str().is_some_and(|s| s.len() <= 100))
                            && values
                                .iter()
                                .all(|v| v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.)),
                        "invalid chart series",
                    )?;
                }
                if e.kind == "table" {
                    let rows = e
                        .data
                        .get("rows")
                        .and_then(Value::as_array)
                        .ok_or("table requires rows")?;
                    require(
                        !rows.is_empty()
                            && rows.len() <= 20
                            && rows.iter().all(|r| {
                                r.as_array().is_some_and(|r| {
                                    !r.is_empty()
                                        && r.len() <= 10
                                        && r.iter()
                                            .all(|v| v.as_str().is_some_and(|s| s.len() <= 300))
                                })
                            }),
                        "invalid table",
                    )?;
                }
                if e.x + e.w > 1280. || e.y + e.h > 720. {
                    diagnostics.push(Diagnostic {
                        code: "OUT_OF_BOUNDS".into(),
                        slide_id: s.id.clone(),
                        element_id: e.id.clone(),
                        message: "Elemento ultrapassa o slide. Reduza ou reposicione.".into(),
                    });
                }
            }
        }
        require(
            serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= MAX_DOCUMENT_BYTES,
            "presentation exceeds 24 MB",
        )?;
        Ok(diagnostics)
    }
}

/// Atomic operation batch: validate before replacing the caller's document.
pub fn apply(document: &Document, operations: &[Value]) -> Result<Document, String> {
    require(
        !operations.is_empty() && operations.len() <= 100,
        "expected 1–100 operations",
    )?;
    let mut doc = document.clone();
    for op in operations {
        let kind = op["type"].as_str().ok_or("missing operation type")?;
        match kind {
            "set_theme" => {
                doc.theme =
                    serde_json::from_value(op["theme"].clone()).map_err(|e| e.to_string())?
            }
            "set_title" => doc.title = op["text"].as_str().ok_or("missing title")?.into(),
            "set_brief" => doc.brief = op["text"].as_str().ok_or("missing brief")?.into(),
            "add_slide" => doc
                .slides
                .push(serde_json::from_value(op["slide"].clone()).map_err(|e| e.to_string())?),
            "remove_slide" => {
                let i = slide_index(&doc, op)?;
                doc.slides.remove(i);
            }
            "move_slide" => {
                let i = slide_index(&doc, op)?;
                let to = op["index"].as_u64().ok_or("missing index")? as usize;
                require(to < doc.slides.len(), "index out of bounds")?;
                let slide = doc.slides.remove(i);
                doc.slides.insert(to, slide);
            }
            "set_transition" => {
                let i = slide_index(&doc, op)?;
                doc.slides[i].transition = op["transition"]
                    .as_str()
                    .ok_or("missing transition")?
                    .into();
            }
            "set_notes" => {
                let i = slide_index(&doc, op)?;
                doc.slides[i].notes = op["text"].as_str().ok_or("missing notes")?.into();
            }
            "add_element" => {
                let i = slide_index(&doc, op)?;
                doc.slides[i].elements.push(
                    serde_json::from_value(op["element"].clone()).map_err(|e| e.to_string())?,
                );
            }
            "set_text" | "update_element" | "remove_element" => {
                let i = slide_index(&doc, op)?;
                let id = op["element_id"].as_str().ok_or("missing element_id")?;
                let j = doc.slides[i]
                    .elements
                    .iter()
                    .position(|e| e.id == id)
                    .ok_or("unknown element_id")?;
                if kind == "remove_element" {
                    doc.slides[i].elements.remove(j);
                } else if kind == "set_text" {
                    doc.slides[i].elements[j].text =
                        op["text"].as_str().ok_or("missing text")?.into();
                } else {
                    let patch = op["patch"].as_object().ok_or("missing patch")?;
                    require(!patch.contains_key("id"), "element id is immutable")?;
                    let mut v = serde_json::to_value(&doc.slides[i].elements[j]).unwrap();
                    for (k, x) in patch {
                        v[k] = x.clone();
                    }
                    doc.slides[i].elements[j] =
                        serde_json::from_value(v).map_err(|e| e.to_string())?;
                }
            }
            _ => return Err(format!("unknown operation: {kind}")),
        }
    }
    doc.validate()?;
    Ok(doc)
}
fn slide_index(doc: &Document, op: &Value) -> Result<usize, String> {
    let id = op["slide_id"].as_str().ok_or("missing slide_id")?;
    doc.slides
        .iter()
        .position(|s| s.id == id)
        .ok_or("unknown slide_id".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_are_atomic() {
        let d = Document::new("test".into(), "Hello".into());
        let result = apply(
            &d,
            &[
                json!({"type":"set_title","text":"Changed"}),
                json!({"type":"remove_slide","slide_id":"slide-1"}),
            ],
        );
        assert!(result.is_err());
        assert_eq!(d.title, "Hello");
    }
    #[test]
    fn images_cannot_reference_network_or_svg() {
        let mut d = Document::new("test".into(), "Hello".into());
        d.assets.push(Asset {
            id: "a".into(),
            name: "x".into(),
            description: String::new(),
            data_url: "data:image/svg+xml;base64,abc".into(),
        });
        assert!(d.validate().is_err());
    }
    #[test]
    fn validates_and_reports_bounds() {
        let mut d = Document::new("test".into(), "Hello".into());
        assert!(d.validate().unwrap().is_empty());
        d.slides[0].elements[0].x = 1200.;
        assert_eq!(d.validate().unwrap()[0].code, "OUT_OF_BOUNDS");
    }
    #[test]
    fn updates_keep_ids() {
        let d = Document::new("test".into(), "Hello".into());
        assert!(apply(&d,&[json!({"type":"update_element","slide_id":"slide-1","element_id":"title-1","patch":{"id":"evil"}})]).is_err());
    }
    #[test]
    fn css_rejects_active_content_and_transition_edits_validate() {
        let mut d = Document::new("test".into(), "Test".into());
        d.css = ".text { background: url\t(https://example.org); }".into();
        assert!(d.validate().is_err());
        d.css = ".text { letter-spacing: 1px; }".into();
        let edited = apply(
            &d,
            &[json!({"type":"set_transition","slide_id":"slide-1","transition":"slide"})],
        )
        .unwrap();
        assert_eq!(edited.slides[0].transition, "slide");
        assert!(apply(
            &d,
            &[json!({"type":"set_transition","slide_id":"slide-1","transition":"unsafe"})]
        )
        .is_err());
    }
}
