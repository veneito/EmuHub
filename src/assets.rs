use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

/// 24x24 outline icons, drawn with black strokes and tinted by the element's text color.
const ICONS: &[(&str, &str)] = &[
    ("home", r#"<path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/>"#),
    ("search", r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#),
    ("library", r#"<path d="M4 4v16M9 4v16M14 6.5l5 13"/>"#),
    ("history", r#"<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>"#),
    ("settings", r#"<path d="M4 7h10M18 7h2M4 17h2M10 17h10"/><circle cx="16" cy="7" r="2"/><circle cx="8" cy="17" r="2"/>"#),
    ("chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#),
    ("chevron-right", r#"<path d="m9 6 6 6-6 6"/>"#),
    ("heart", r#"<path d="M12 20s-7-4.5-9-9.2C1.8 7.5 4 4.5 7 4.5c2 0 3.5 1.2 5 3 1.5-1.8 3-3 5-3 3 0 5.2 3 4 6.3-2 4.7-9 9.2-9 9.2z"/>"#),
    ("heart-filled", r#"<path fill="black" d="M12 20s-7-4.5-9-9.2C1.8 7.5 4 4.5 7 4.5c2 0 3.5 1.2 5 3 1.5-1.8 3-3 5-3 3 0 5.2 3 4 6.3-2 4.7-9 9.2-9 9.2z"/>"#),
    ("play", r#"<path fill="black" d="M7 4.5v15l12-7.5z"/>"#),
    ("panel-right", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>"#),
    ("sidebar", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>"#),
    ("folder", r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#),
    ("terminal", r#"<path d="m5 8 5 4-5 4M12 17h7"/>"#),
    ("image", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="m21 16-5-5-9 9"/>"#),
    ("arrow-left", r#"<path d="M19 12H5m6-6-6 6 6 6"/>"#),
    ("refresh", r#"<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>"#),
    // Controller button glyphs for the hints bar.
    ("btn-cross", r#"<path d="m7 7 10 10M17 7 7 17"/>"#),
    ("btn-circle", r#"<circle cx="12" cy="12" r="6.5"/>"#),
    ("btn-menu", r#"<path d="M6 8h12M6 12h12M6 16h12"/>"#),
];

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let icon = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"))
            .and_then(|name| ICONS.iter().find(|(known, _)| *known == name));
        if let Some((_, inner)) = icon {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{inner}</svg>"#
            );
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        let data: &[u8] = match path {
            "icons/window-minimize.svg" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3 8h10" fill="none" stroke="black" stroke-width="1.2"/></svg>"#,
            "icons/window-maximize.svg" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="3.5" y="3.5" width="9" height="9" fill="none" stroke="black" stroke-width="1.2"/></svg>"#,
            "icons/window-restore.svg" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M5 3.5h7.5V11M3.5 5.5h7v7h-7z" fill="none" stroke="black" stroke-width="1.2"/></svg>"#,
            "banner.png" => include_bytes!("../assets/banner.png"),
            "icons/window-close.svg" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m4 4 8 8m0-8-8 8" fill="none" stroke="black" stroke-width="1.2"/></svg>"#,
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(data)))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
