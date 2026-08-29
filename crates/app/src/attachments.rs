//! Naming a file that arrived without a name.
//!
//! An attachment reaches the Director as a **path**: `director::with_attachments`
//! writes the paths into the message and tells the agent to read them with its
//! own tools. That works for a picked file, which already has a path and a name
//! its owner chose.
//!
//! A pasted screenshot has neither. The clipboard hands over bytes and a MIME
//! type, and nothing else — no name, no extension, no folder. So before it can
//! be attached it has to become a real file, and the name it gets is what the
//! operator will read back in the transcript months later. That is what this
//! module decides.

/// Refuse anything larger than this. The clipboard will happily hand over a
/// multi-megabyte capture of a 6K display, and every byte of it is copied into
/// the message the model is billed for reading.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// The extension for a clipboard MIME type. `None` for anything not worth
/// writing to disk under a name that claims to know what it is.
pub fn extension_for(mime: &str) -> Option<&'static str> {
    let mime = mime.trim().to_ascii_lowercase();
    Some(match mime.split(';').next().unwrap_or("").trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/markdown" => "md",
        "text/csv" => "csv",
        "application/json" => "json",
        _ => return None,
    })
}

/// Keep what a file manager would show and drop the rest. Nothing here may
/// traverse: a clipboard name is not trusted input, and `..` in it would put
/// the write wherever it liked.
fn clean_stem(name: &str) -> String {
    let stem = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('-').to_string();
    // 60 is long enough to stay recognisable and short enough that the path
    // survives being nested under a data directory somebody moved.
    cleaned.chars().take(60).collect::<String>().trim().to_string()
}

/// What to call the file. `stamp_ms` makes it unique without a counter, and
/// makes the transcript's order legible from the filenames alone.
///
/// A pasted image has no name at all, so it gets one that says what it is and
/// when it happened — "pasted-1724930400000.png" — rather than a hash, which
/// tells the operator nothing when they meet it again in six months.
pub fn file_name(original: Option<&str>, mime: &str, stamp_ms: u64) -> Option<String> {
    let ext = extension_for(mime)?;
    let stem = original.map(clean_stem).filter(|s| !s.is_empty());
    Some(match stem {
        Some(stem) => format!("{stem}-{stamp_ms}.{ext}"),
        None => format!("pasted-{stamp_ms}.{ext}"),
    })
}

/// Why this cannot be attached, or `None` if it can.
pub fn refuse(mime: &str, len: usize) -> Option<String> {
    if len == 0 {
        return Some("that came through empty".to_string());
    }
    if len > MAX_BYTES {
        return Some(format!(
            "that is {} MB — the ceiling for an attachment is {} MB",
            len / (1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    if extension_for(mime).is_none() {
        return Some(format!("Relay has no name for a {mime} — save it and attach the file"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_image_is_named_for_what_it_is_and_when() {
        assert_eq!(
            file_name(None, "image/png", 1_724_930_400_000).unwrap(),
            "pasted-1724930400000.png"
        );
    }

    #[test]
    fn a_dropped_file_keeps_the_name_its_owner_gave_it() {
        assert_eq!(
            file_name(Some("Screen Shot.png"), "image/png", 7).unwrap(),
            "Screen Shot-7.png"
        );
    }

    #[test]
    fn a_name_cannot_traverse_out_of_the_directory() {
        // The clipboard is not a trusted source of paths.
        let got = file_name(Some("../../../../etc/passwd"), "image/png", 1).unwrap();
        assert!(!got.contains(".."), "{got}");
        assert!(!got.contains('/'), "{got}");
        assert_eq!(got, "passwd-1.png");
    }

    #[test]
    fn the_mime_decides_the_extension_not_the_name() {
        // A clipboard that says PNG and a name that says .exe: believe the MIME.
        let got = file_name(Some("payload.exe"), "image/png", 3).unwrap();
        assert_eq!(got, "payload-3.png");
    }

    #[test]
    fn a_parameterised_mime_still_resolves() {
        assert_eq!(extension_for("text/plain; charset=utf-8"), Some("txt"));
    }

    #[test]
    fn an_unknown_type_is_refused_rather_than_guessed() {
        assert!(file_name(None, "application/x-msdownload", 1).is_none());
        assert!(refuse("application/x-msdownload", 10).is_some());
    }

    #[test]
    fn empty_and_oversized_are_both_refused_by_name() {
        assert!(refuse("image/png", 0).unwrap().contains("empty"));
        assert!(refuse("image/png", MAX_BYTES + 1).unwrap().contains("ceiling"));
        assert!(refuse("image/png", 1024).is_none());
    }

    #[test]
    fn a_name_that_cleans_to_nothing_falls_back_rather_than_producing_a_dotfile() {
        assert_eq!(file_name(Some("---"), "image/png", 5).unwrap(), "pasted-5.png");
        assert_eq!(file_name(Some(""), "image/png", 5).unwrap(), "pasted-5.png");
    }
}
