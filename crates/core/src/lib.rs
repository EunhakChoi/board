//! Pure domain rules shared by the server. No I/O lives here.

/// Product version, shared by the server and reported to clients.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert_ne!(super::VERSION, "");
    }
}
