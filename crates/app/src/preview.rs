//! Showing a picture that lives on disk.
//!
//! An agent that generates an image gets back a path and writes it into its
//! answer as markdown. The webview cannot open that path: it is a page served
//! from a custom protocol, and a bare `/Users/…/x.png` in an `<img>` resolves
//! against the page, not against the filesystem. So the bytes come across the
//! IPC boundary as a data URL, which is the same road a pasted attachment
//! already travels (`attachments.rs`) — and it means the CSP stays as it is,
//! with no asset protocol opened up and no directory glob in a config file
//! that nothing tests.
//!
//! What *is* tested is this: which paths may be read at all. A transcript is
//! written by a model, so the path in it is model-supplied text. Rendering it
//! without a fence would make an `<img>` in an agent's answer a way to read any
//! file on the machine into the window.

use std::path::{Path, PathBuf};

/// The biggest image worth inlining. A generated PNG runs to a couple of
/// megabytes; base64 makes that a third bigger again, and it is held as a
/// string in the webview for as long as the message is on screen. Past this,
/// the answer is a link the operator opens in their own viewer rather than a
/// picture that makes the window crawl.
pub const MAX_BYTES: u64 = 12 * 1024 * 1024;

/// The MIME for a file we are willing to draw, by extension.
///
/// By extension and not by sniffing the bytes, because the extension is what
/// decides how the webview treats it: a PNG renamed `.svg` would be handed over
/// as `image/svg+xml`, and an SVG is a document that can carry script. So SVG
/// is deliberately **not** here — an agent-supplied SVG inlined into the window
/// is a script tag with extra steps, and every image Relay itself produces is a
/// raster one.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}

/// May this path be read into the window?
///
/// Inside one of `roots`, and nowhere else. The comparison is done on the
/// canonical path so `..` cannot walk out of a root that the string prefix
/// alone would have accepted — which is the whole reason this is a function
/// with a test rather than a `starts_with` at the call site.
///
/// A path that cannot be canonicalised is refused. That covers the file not
/// existing, which is the ordinary case for a model that invented a filename.
pub fn readable(path: &Path, roots: &[PathBuf]) -> bool {
    if mime_for(path).is_none() {
        return false;
    }
    let Ok(real) = path.canonicalize() else {
        return false;
    };
    if !real.is_file() {
        return false;
    }
    roots.iter().any(|root| {
        root.canonicalize()
            .map(|root| real.starts_with(root))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"not really a png").unwrap();
    }

    #[test]
    fn only_raster_images_have_a_type_and_svg_deliberately_does_not() {
        assert_eq!(mime_for(Path::new("a/b.png")), Some("image/png"));
        assert_eq!(mime_for(Path::new("a/b.JPEG")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a/b.webp")), Some("image/webp"));
        // An SVG inlined into the window is a document that can carry script.
        assert_eq!(mime_for(Path::new("a/b.svg")), None);
        assert_eq!(mime_for(Path::new("a/b.txt")), None);
        assert_eq!(mime_for(Path::new("a/b")), None);
    }

    /// The fence itself. The path comes out of a transcript a model wrote, so
    /// "inside a root" has to survive somebody writing `..` into it.
    #[test]
    fn a_path_may_be_read_only_from_inside_a_root_and_traversal_does_not_help() {
        let base = std::env::temp_dir().join(format!("relay-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let allowed = base.join("allowed");
        let secret = base.join("secret");
        touch(&allowed.join("shot.png"));
        touch(&secret.join("keys.png"));
        let roots = vec![allowed.clone()];

        assert!(readable(&allowed.join("shot.png"), &roots));
        assert!(!readable(&secret.join("keys.png"), &roots));
        assert!(
            !readable(&allowed.join("../secret/keys.png"), &roots),
            "a prefix check on the unresolved path would have let this through"
        );
        assert!(!readable(&allowed.join("missing.png"), &roots), "a name a model invented");
        assert!(!readable(&allowed.join("shot.png"), &[]), "no roots, nothing readable");

        // A directory is not an image however it is spelled.
        std::fs::create_dir_all(allowed.join("folder.png")).unwrap();
        assert!(!readable(&allowed.join("folder.png"), &roots));

        let _ = std::fs::remove_dir_all(&base);
    }
}
