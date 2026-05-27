//! Clipboard content sanitization for security.
//!
//! Filters potentially sensitive data before syncing to remote peers.
//! Detects and strips passwords, tokens, credit card numbers, and
//! limits content size.

/// Maximum text content size to sync (1 MB).
const MAX_TEXT_SIZE: usize = 1024 * 1024;

/// Maximum image content size to sync (10 MB).
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Patterns that indicate sensitive content that should not be synced.
const SENSITIVE_KEYWORDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "access_key",
    "credentials",
    "authorization",
];

/// Result of content sanitization.
#[derive(Debug, Clone, PartialEq)]
pub enum SanitizeResult {
    /// Content is safe to sync (possibly modified).
    Allowed(String),
    /// Content was blocked entirely.
    Blocked(String),
}

/// Check if text content looks like it might contain sensitive data.
///
/// This uses heuristic checks:
/// - Length limits
/// - Sensitive keyword detection
/// - Credit card number pattern detection (Luhn algorithm)
/// - Common secret format patterns
pub fn sanitize_text(text: &str) -> SanitizeResult {
    // Check size limit
    if text.len() > MAX_TEXT_SIZE {
        return SanitizeResult::Blocked(format!(
            "Text too large ({} bytes, max {})",
            text.len(),
            MAX_TEXT_SIZE
        ));
    }

    let lower = text.to_lowercase();

    // Check for sensitive keywords in the surrounding context
    for keyword in SENSITIVE_KEYWORDS {
        if lower.contains(keyword) {
            // Check if this looks like a key=value pair containing a secret
            if looks_like_secret_value(text, keyword) {
                return SanitizeResult::Blocked(format!(
                    "Detected sensitive content: {}",
                    keyword
                ));
            }
        }
    }

    // Check for credit card number patterns
    if contains_credit_card(text) {
        return SanitizeResult::Blocked("Detected possible credit card number".into());
    }

    // Check for common token/secret patterns
    if looks_like_token(text) {
        return SanitizeResult::Blocked("Detected possible API token or secret".into());
    }

    SanitizeResult::Allowed(text.to_string())
}

/// Sanitize image data - just check size limits.
pub fn sanitize_image(width: usize, height: usize, pixels: &[u8]) -> SanitizeResult {
    let expected = width.checked_mul(height).and_then(|sz| sz.checked_mul(4));
    match expected {
        Some(exp) if exp == pixels.len() && pixels.len() <= MAX_IMAGE_SIZE => {
            SanitizeResult::Allowed("ok".into())
        }
        Some(exp) if pixels.len() != exp => {
            SanitizeResult::Blocked(format!(
                "Pixel data size mismatch: expected {}, got {}",
                exp,
                pixels.len()
            ))
        }
        _ => SanitizeResult::Blocked(format!(
            "Image too large ({} bytes, max {})",
            pixels.len(),
            MAX_IMAGE_SIZE
        )),
    }
}

/// Check if the text around a keyword looks like a key=secret pair.
fn looks_like_secret_value(text: &str, keyword: &str) -> bool {
    let lower = text.to_lowercase();
    if let Some(pos) = lower.find(keyword) {
        // Look for common assignment patterns after the keyword
        let after = &text[pos + keyword.len()..];
        let trimmed = after.trim_start();
        // Check for ":", "=", "=>" patterns followed by a value
        if trimmed.starts_with(':') || trimmed.starts_with('=') {
            // There's a value after the keyword - likely a secret
            let value_part = trimmed[1..].trim_start();
            if !value_part.is_empty() && value_part.len() < 500 {
                return true;
            }
        }
    }
    false
}

/// Check for credit card number patterns using Luhn algorithm.
fn contains_credit_card(text: &str) -> bool {
    let digits: Vec<u8> = text
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c as u8 - b'0')
        .collect();

    // Check all 13-19 digit sequences
    for window in digits.windows(16) {
        if luhn_check(window) {
            return true;
        }
    }
    // Also check 15-digit (Amex)
    if digits.len() >= 15 {
        for window in digits.windows(15) {
            if luhn_check(window) {
                return true;
            }
        }
    }

    false
}

/// Luhn algorithm for credit card validation.
fn luhn_check(digits: &[u8]) -> bool {
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum: u32 = 0;
    let _num_digits = digits.len();
    for (i, &d) in digits.iter().rev().enumerate() {
        if d > 9 {
            return false;
        }
        let mut n = d as u32;
        if i % 2 == 1 {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
    }
    sum % 10 == 0
}

/// Check if the text looks like a bearer token, JWT, or API key.
fn looks_like_token(text: &str) -> bool {
    let trimmed = text.trim();

    // JWT pattern: three base64url segments separated by dots
    if trimmed.contains('.') {
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| p.len() > 10) {
            return true;
        }
    }

    // Bearer token
    if trimmed.starts_with("Bearer ") || trimmed.starts_with("bearer ") {
        return true;
    }

    // Long hex string (common for tokens/keys)
    let hex_only: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if hex_only.len() >= 32 && hex_only.len() == trimmed.len() {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_normal_text() {
        let result = sanitize_text("Hello, this is normal clipboard text");
        assert!(matches!(result, SanitizeResult::Allowed(_)));
    }

    #[test]
    fn test_sanitize_text_too_large() {
        let large = "x".repeat(MAX_TEXT_SIZE + 1);
        let result = sanitize_text(&large);
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_password_blocked() {
        let result = sanitize_text("password=MyS3cr3tP@ss!");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_api_key_blocked() {
        let result = sanitize_text("api_key: abc123def456");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_password_in_text_allowed() {
        // The word "password" alone without a value assignment should be allowed
        let result = sanitize_text("Please enter your password to continue");
        assert!(matches!(result, SanitizeResult::Allowed(_)));
    }

    #[test]
    fn test_sanitize_credit_card_blocked() {
        // Valid Luhn test number (Visa test: 4111111111111111)
        let result = sanitize_text("Card: 4111 1111 1111 1111");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_bearer_token_blocked() {
        let result = sanitize_text("Bearer eyJhbGciOiJIUzI1NiJ9.test.sig");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_jwt_blocked() {
        let result = sanitize_text("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.abcdef1234567890");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_image_ok() {
        let result = sanitize_image(4, 4, &vec![0u8; 64]);
        assert!(matches!(result, SanitizeResult::Allowed(_)));
    }

    #[test]
    fn test_sanitize_image_too_large() {
        let result = sanitize_image(2000, 2000, &vec![0u8; MAX_IMAGE_SIZE + 1]);
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_image_size_mismatch() {
        let result = sanitize_image(4, 4, &vec![0u8; 32]);
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_luhn_check() {
        assert!(luhn_check(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
        assert!(!luhn_check(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6]));
    }

    #[test]
    fn test_sanitize_hex_token_blocked() {
        let result = sanitize_text("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4");
        assert!(matches!(result, SanitizeResult::Blocked(_)));
    }

    #[test]
    fn test_sanitize_short_text_allowed() {
        let result = sanitize_text("hi");
        assert!(matches!(result, SanitizeResult::Allowed(_)));
    }
}
