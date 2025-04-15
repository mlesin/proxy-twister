use regex::Regex;

/// Convert a simple wildcard pattern (only '*' supported) to a Regex
///
/// If the pattern is in the form "*.domain.com", it will also match "domain.com"
fn wildcard_to_regex(pattern: &str) -> Regex {
    if let Some(domain) = pattern.strip_prefix("*.") {
        // For patterns like "*.example.com", also match "example.com"
        let regex_string = format!("^(.*\\.)?{}$", regex::escape(domain));
        Regex::new(&regex_string).expect("Invalid regex")
    } else {
        // Handle other patterns as before
        let regex_string = "^".to_string() + &regex::escape(pattern).replace("\\*", ".*") + "$";
        Regex::new(&regex_string).expect("Invalid regex")
    }
}

/// Check if a hostname matches a wildcard pattern
pub fn matches_pattern(host: &str, pattern: &str) -> bool {
    let re = wildcard_to_regex(pattern);
    re.is_match(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_matching() {
        assert!(matches_pattern("abc.discord.gg", "*.discord.gg"));
        assert!(matches_pattern("xyz.discord.com", "*.discord.com"));
        assert!(!matches_pattern("test.instagram.com", "*.discord.com"));

        // Test that root domains match with wildcard patterns
        assert!(matches_pattern("discord.gg", "*.discord.gg"));
        assert!(matches_pattern("example.com", "*.example.com"));

        // Test that regular patterns still work
        assert!(matches_pattern("exact.match", "exact.match"));
        assert!(matches_pattern("test.wildcard.match", "test.*.match"));
        assert!(!matches_pattern("wrong.wildcard.com", "test.*.com"));
    }
}
