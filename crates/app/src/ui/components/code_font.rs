//! The face fenced code should be set in.

/// **Nothing is bundled.** A monospace font is a multi-megabyte binary, a licence to carry and
/// a rendering result nobody here can look at — and it buys nothing on the platform that
/// matters, because `Consolas` has shipped with every Windows since Vista and `Cascadia Mono`
/// with every Windows 11. So this is a stack, not a file: the nice one first, the one that is
/// certainly present next, and a Linux name for the development machine.
///
/// `HighlightStyle` carries no font family, so this reaches fenced blocks only. *Inline* code
/// stays marked by colour, which is what it already was.
pub fn code_font() -> gpui::Font {
    // The *primary* is chosen per platform and is one that is always installed there, because
    // `fallbacks` covers missing glyphs and is not a promise about a missing family. Naming a
    // font that might not exist risks a block that renders as nothing, and nothing is exactly
    // what a missing code block looks like.
    let (family, rest) = if cfg!(target_os = "windows") {
        ("Consolas", vec!["Cascadia Mono", "Courier New"])
    } else if cfg!(target_os = "macos") {
        ("Menlo", vec!["Monaco", "Courier New"])
    } else {
        ("DejaVu Sans Mono", vec!["Liberation Mono", "monospace"])
    };
    gpui::Font {
        family: family.into(),
        features: gpui::FontFeatures::default(),
        fallbacks: Some(gpui::FontFallbacks::from_fonts(
            rest.into_iter().map(str::to_string).collect(),
        )),
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    }
}
