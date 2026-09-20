//! Comparing secrets. One copy, shared by `serve` (desktop only) and `devlog_admin` (also built
//! for Android): a second hand-rolled copy of anything timing-sensitive is a second place for it
//! to quietly stop being constant-time.
//!
//! This used to live in `serve`, and `devlog_admin` reached it through `crate::serve` — which does
//! not exist on Android, so the Android library failed to compile ("cannot find `serve` in the
//! crate root"). A shared helper cannot sit inside a module gated tighter than its users.

/// Compare without leaking where the two differ through timing. Length is not a secret here (the
/// user chose it), but the content is.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn the_token_check_is_exact_and_length_safe() {
        assert!(constant_time_eq("hunter2", "hunter2"));
        assert!(!constant_time_eq("hunter2", "hunter3"));
        assert!(!constant_time_eq("hunter2", "hunter22"));
        assert!(!constant_time_eq("", "x"));
        // An empty configured token means the server is open; that decision is made before this
        // is ever called, but two empties must not compare unequal.
        assert!(constant_time_eq("", ""));
    }
}
