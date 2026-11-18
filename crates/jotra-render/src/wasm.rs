#![cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

#[derive(serde::Serialize)]
struct JsFragment {
    html: String,
    has_mermaid: bool,
    has_math: bool,
}

#[wasm_bindgen]
pub fn render_fragment(text: &str) -> JsValue {
    let f = crate::render_markdown_fragment(text);
    serde_wasm_bindgen::to_value(&JsFragment {
        html: f.html,
        has_mermaid: f.has_mermaid,
        has_math: f.has_math,
    })
    .unwrap_or(JsValue::NULL)
}
