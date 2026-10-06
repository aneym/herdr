use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(crate) fn truncate_end(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let prefix = take_prefix_width(text, max_width.saturating_sub(1));
    format!("{prefix}…")
}

/// Whole graphemes only: an emoji with its variation selector or ZWJ parts is never
/// split, and each counts at the width the terminal draws it.
pub(crate) fn take_prefix_width(text: &str, max_width: usize) -> String {
    let mut output = String::new();
    let mut width = 0usize;
    for grapheme in text.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if width + grapheme_width > max_width {
            break;
        }
        output.push_str(grapheme);
        width += grapheme_width;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_end_uses_display_width() {
        let text = truncate_end("提交 herdr 的反馈", 16);

        assert_eq!(text, "提交 herdr 的反…");
        assert!(display_width(&text) <= 16);
    }
}
