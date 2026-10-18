//! Domain model: projects, versions, branches, comments, anchors, screens and flows. Pure data, no I/O.

/// Product version, reported by every app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert_ne!(super::VERSION, "");
    }
}
