#[cfg(test)]
mod tests {
    use crate::event_store::initializer::OpossumOptions;
    use alloc::string::ToString;
    

    #[cfg(feature = "std")]
    fn get_valid_absolute_path() -> alloc::string::String {
        if cfg!(windows) {
            "C:\\TestPath".to_string()
        } else {
            "/tmp/TestPath".to_string()
        }
    }

    #[cfg(not(feature = "std"))]
    fn get_valid_absolute_path() -> alloc::string::String {
        "/tmp/TestPath".to_string()
    }

    #[cfg(feature = "std")]
    fn get_path_with_invalid_characters() -> alloc::string::String {
        if cfg!(windows) {
            "C:\\Invalid|Path".to_string()
        } else {
            "/tmp/Invalid\0Path".to_string()
        }
    }

    #[cfg(not(feature = "std"))]
    fn get_path_with_invalid_characters() -> alloc::string::String {
        "/tmp/Invalid\0Path".to_string()
    }

    #[test]
    fn validate_valid_options_returns_success() {
        let mut options = OpossumOptions::new(get_valid_absolute_path());
        options = options.use_store("ValidContext");

        let result = options.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn validate_empty_root_path_returns_fail() {
        let mut options = OpossumOptions::new("");
        options = options.use_store("ValidContext");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.iter().any(|f| f.contains("cannot be null or empty")));
    }

    #[test]
    fn validate_relative_path_returns_fail() {
        let mut options = OpossumOptions::new("relative/path");
        options = options.use_store("ValidContext");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.iter().any(|f| f.contains("must be an absolute path")));
    }

    #[test]
    fn validate_invalid_path_characters_returns_fail() {
        let mut options = OpossumOptions::new(get_path_with_invalid_characters());
        options = options.use_store("ValidContext");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.iter().any(|f| f.contains("invalid characters")));
    }

    #[test]
    fn validate_no_store_name_returns_fail() {
        let options = OpossumOptions::new(get_valid_absolute_path());

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.iter().any(|f| f.contains("StoreName must be configured")));
    }

    #[test]
    #[should_panic(expected = "name cannot be empty")]
    fn use_store_with_empty_name_throws() {
        let options = OpossumOptions::new(get_valid_absolute_path());
        let _ = options.use_store("");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws() {
        let options = OpossumOptions::new(get_valid_absolute_path());
        let _ = options.use_store("Invalid/Name");
    }

    #[test]
    fn use_store_with_reserved_name_fails_validation() {
        let mut options = OpossumOptions::new(get_valid_absolute_path());
        options = options.use_store("CON");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.iter().any(|f| f.contains("Invalid store name 'CON'")));
    }

    #[test]
    fn validate_multiple_failures_returns_all_errors() {
        let options = OpossumOptions::new("relative/path");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(failures.len() >= 2, "Should report both RootPath and StoreName failures");
        assert!(failures.iter().any(|f| f.contains("must be an absolute path")));
        assert!(failures.iter().any(|f| f.contains("StoreName must be configured")));
    }
}
