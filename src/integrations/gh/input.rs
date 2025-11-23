#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
/// Represents the type of branch input provided by the user
#[derive(Debug, PartialEq, Eq)]
pub enum BranchInput {
    /// GitHub issue or PR number (e.g., "#123")
    Github(u32),
    /// Plain branch name
    Plain(String),
}

impl BranchInput {
    /// Parse a branch name string into a `BranchInput`
    ///
    /// Recognizes `#123` pattern as GitHub issue/PR number.
    /// Everything else is treated as a plain branch name.
    pub fn parse(input: &str) -> Self {
        if let Some(stripped) = input.strip_prefix('#') {
            if let Ok(number) = stripped.parse::<u32>() {
                return Self::Github(number);
            }
        }
        Self::Plain(input.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_number() {
        let result = BranchInput::parse("#123");
        assert_eq!(result, BranchInput::Github(123));
    }

    #[test]
    fn test_parse_plain_branch() {
        let result = BranchInput::parse("feature-branch");
        assert_eq!(result, BranchInput::Plain("feature-branch".to_string()));
    }

    #[test]
    fn test_parse_branch_with_hash_in_middle() {
        let result = BranchInput::parse("feature-#42");
        assert_eq!(result, BranchInput::Plain("feature-#42".to_string()));
    }

    #[test]
    fn test_parse_single_digit() {
        let result = BranchInput::parse("#1");
        assert_eq!(result, BranchInput::Github(1));
    }

    #[test]
    fn test_parse_large_number() {
        let result = BranchInput::parse("#99999");
        assert_eq!(result, BranchInput::Github(99999));
    }
}
