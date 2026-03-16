#[derive(Debug, Clone)]
pub struct OpossumOptions {
    pub root_path: alloc::string::String,
    pub store_name: Option<alloc::string::String>,
    pub flush_events_immediately: bool,
}

impl OpossumOptions {
    pub fn new(root_path: impl Into<alloc::string::String>) -> Self {
        Self {
            root_path: root_path.into(),
            store_name: None,
            flush_events_immediately: true,
        }
    }

    pub fn use_store(mut self, name: impl Into<alloc::string::String>) -> Self {
        if self.store_name.is_some() {
            panic!("UseStore has already been called");
        }
        let name_str = name.into();
        if name_str.trim().is_empty() {
            panic!("name cannot be empty");
        }
        if name_str.contains('/')
            || name_str.contains('\\')
            || name_str.contains(':')
            || name_str.contains('*')
            || name_str.contains('?')
        {
            panic!("Invalid store name");
        }
        self.store_name = Some(name_str);
        self
    }

    pub fn validate(&self) -> Result<(), alloc::vec::Vec<alloc::string::String>> {
        let mut failures = alloc::vec::Vec::new();
        if self.root_path.trim().is_empty() {
            failures.push("RootPath cannot be null or empty".into());
        } else {
            #[cfg(feature = "std")]
            {
                let path = std::path::Path::new(&self.root_path);
                if !path.is_absolute() {
                    failures.push("RootPath must be an absolute path".into());
                }
            }
            #[cfg(not(feature = "std"))]
            {
                if !self.root_path.starts_with('/') && self.root_path.chars().nth(1) != Some(':') {
                    failures.push("RootPath must be an absolute path".into());
                }
            }
            if self.root_path.contains('|') || self.root_path.contains('\0') {
                failures.push("RootPath contains invalid characters".into());
            }
        }
        if let Some(ref name) = self.store_name {
            if name.eq_ignore_ascii_case("CON") || name.eq_ignore_ascii_case("PRN") {
                failures.push(alloc::format!("Invalid store name '{}'", name));
            }
        } else {
            failures.push("StoreName must be configured".into());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(
            failures
                .iter()
                .any(|f| f.contains("cannot be null or empty"))
        );
    }

    #[test]
    fn validate_relative_path_returns_fail() {
        let mut options = OpossumOptions::new("relative/path");
        options = options.use_store("ValidContext");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(
            failures
                .iter()
                .any(|f| f.contains("must be an absolute path"))
        );
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
        assert!(
            failures
                .iter()
                .any(|f| f.contains("StoreName must be configured"))
        );
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
        assert!(
            failures
                .iter()
                .any(|f| f.contains("Invalid store name 'CON'"))
        );
    }

    #[test]
    fn validate_multiple_failures_returns_all_errors() {
        let options = OpossumOptions::new("relative/path");

        let result = options.validate();
        assert!(result.is_err());
        let failures = result.unwrap_err();
        assert!(
            failures.len() >= 2,
            "Should report both RootPath and StoreName failures"
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("must be an absolute path"))
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("StoreName must be configured"))
        );
    }

    #[test]
    fn constructor_sets_default_root_path() {
        let options = OpossumOptions::new("OpossumStore");
        assert_eq!(options.root_path, "OpossumStore");
    }

    #[test]
    fn constructor_store_name_is_null_by_default() {
        let options = OpossumOptions::new("OpossumStore");
        assert!(options.store_name.is_none());
    }

    #[test]
    fn use_store_with_valid_name_sets_store_name() {
        let options = OpossumOptions::new("root").use_store("CourseManagement");
        assert_eq!(options.store_name.unwrap(), "CourseManagement");
    }

    #[test]
    #[should_panic(expected = "UseStore has already been called")]
    fn use_store_when_called_twice_throws_invalid_operation_exception() {
        let _ = OpossumOptions::new("root")
            .use_store("CourseManagement")
            .use_store("Billing");
    }

    #[test]
    #[should_panic(expected = "name cannot be empty")]
    fn use_store_with_whitespace_name_throws_argument_exception() {
        let _ = OpossumOptions::new("root").use_store("   ");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_slash() {
        let _ = OpossumOptions::new("root").use_store("Course/Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_backslash() {
        let _ = OpossumOptions::new("root").use_store("Course\\Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_colon() {
        let _ = OpossumOptions::new("root").use_store("Course:Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_star() {
        let _ = OpossumOptions::new("root").use_store("Course*Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_question() {
        let _ = OpossumOptions::new("root").use_store("Course?Management");
    }

    #[test]
    #[should_panic(expected = "UseStore has already been called")]
    fn use_store_when_called_twice_with_same_name_throws() {
        let _ = OpossumOptions::new("root")
            .use_store("CourseManagement")
            .use_store("CourseManagement");
    }

    #[test]
    fn root_path_can_be_set() {
        let mut options = OpossumOptions::new("");
        options.root_path = "/custom/path/to/store".to_string();
        assert_eq!(options.root_path, "/custom/path/to/store");
    }

    #[test]
    fn root_path_can_be_set_to_relative_path() {
        let mut options = OpossumOptions::new("");
        options.root_path = "./data/events".to_string();
        assert_eq!(options.root_path, "./data/events");
    }

    #[test]
    fn use_store_with_valid_names_sets_store_name_various() {
        let valid_names = [
            "ValidContext",
            "Context123",
            "Context_With_Underscores",
            "Context-With-Dashes",
            "Context.With.Dots",
            "CourseManagement",
        ];
        for name in valid_names {
            let options = OpossumOptions::new("root").use_store(name);
            assert_eq!(options.store_name.unwrap(), name);
        }
    }

    #[test]
    fn constructor_sets_flush_events_immediately_to_true_by_default() {
        let options = OpossumOptions::new("root");
        assert!(options.flush_events_immediately);
    }

    #[test]
    fn flush_events_immediately_can_be_set_to_false() {
        let mut options = OpossumOptions::new("root");
        options.flush_events_immediately = false;
        assert!(!options.flush_events_immediately);
    }

    #[test]
    fn flush_events_immediately_can_be_set_to_true() {
        let mut options = OpossumOptions::new("root");
        options.flush_events_immediately = false;
        options.flush_events_immediately = true;
        assert!(options.flush_events_immediately);
    }
}
