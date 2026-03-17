use alloc::string::String;
use core::fmt;

#[derive(Debug, PartialEq, Clone)]
pub enum EventStoreError {
    /// Thrown when an append condition fails.
    AppendConditionFailed(String),

    /// Thrown when a context is not found.
    ContextNotFound {
        message: String,
        context_name: Option<String>,
    },

    /// Thrown when a query is invalid.
    InvalidQuery(String),

    /// Thrown when there is a concurrency conflict.
    Concurrency {
        message: String,
        expected_sequence: Option<u64>,
        actual_sequence: Option<u64>,
    },

    /// Thrown when an event could not be found.
    EventNotFound {
        message: String,
        query_description: Option<String>,
    },

    /// Base or default generic event store error
    General(String),
}

impl fmt::Display for EventStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppendConditionFailed(msg) => write!(f, "{}", msg),
            Self::ContextNotFound { message, .. } => write!(f, "{}", message),
            Self::InvalidQuery(msg) => write!(f, "{}", msg),
            Self::Concurrency { message, .. } => write!(f, "{}", message),
            Self::EventNotFound { message, .. } => write!(f, "{}", message),
            Self::General(msg) => write!(f, "{}", msg),
        }
    }
}

// In Rust, we use Traits or methods to simulate the subclassing polymorphism of C#
impl EventStoreError {
    pub fn is_append_condition_failure(&self) -> bool {
        matches!(
            self,
            Self::AppendConditionFailed(_) | Self::Concurrency { .. }
        )
    }
}

impl core::error::Error for EventStoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_store_error_general_creates_exception() {
        let exception = EventStoreError::General("Test error message".to_string());
        assert_eq!(exception.to_string(), "Test error message");
    }

    #[test]
    fn append_condition_failed_error_creates_exception() {
        let message = "Append condition failed: conflicting events found".to_string();
        let exception = EventStoreError::AppendConditionFailed(message.clone());

        assert_eq!(exception.to_string(), message);
        assert!(exception.is_append_condition_failure());
    }

    #[test]
    fn context_not_found_error_sets_properties() {
        let message = "Context 'Billing' not found".to_string();
        let context_name = Some("Billing".to_string());

        let exception = EventStoreError::ContextNotFound {
            message: message.clone(),
            context_name: context_name.clone(),
        };

        assert_eq!(exception.to_string(), message);
        if let EventStoreError::ContextNotFound {
            context_name: c_name,
            ..
        } = exception
        {
            assert_eq!(c_name, context_name);
        } else {
            panic!("Expected ContextNotFound");
        }
    }

    #[test]
    fn context_not_found_error_no_context_name() {
        let message = "Context not found".to_string();
        let exception = EventStoreError::ContextNotFound {
            message: message.clone(),
            context_name: None,
        };

        assert_eq!(exception.to_string(), message);
        if let EventStoreError::ContextNotFound { context_name, .. } = exception {
            assert_eq!(context_name, None);
        } else {
            panic!("Expected ContextNotFound");
        }
    }

    #[test]
    fn invalid_query_error_creates_exception() {
        let message = "Query must contain at least one QueryItem".to_string();
        let exception = EventStoreError::InvalidQuery(message.clone());
        assert_eq!(exception.to_string(), message);
    }

    #[test]
    fn concurrency_error_creates_exception() {
        let message = "Concurrency conflict detected".to_string();
        let exception = EventStoreError::Concurrency {
            message: message.clone(),
            expected_sequence: None,
            actual_sequence: None,
        };
        assert_eq!(exception.to_string(), message);
        assert!(exception.is_append_condition_failure());
    }

    #[test]
    fn concurrency_error_with_sequences_creates_exception() {
        let message = "Ledger sequence conflict".to_string();
        let exception = EventStoreError::Concurrency {
            message: message.clone(),
            expected_sequence: Some(42),
            actual_sequence: Some(43),
        };

        assert_eq!(exception.to_string(), message);
        if let EventStoreError::Concurrency {
            expected_sequence,
            actual_sequence,
            ..
        } = exception
        {
            assert_eq!(expected_sequence, Some(42));
            assert_eq!(actual_sequence, Some(43));
        } else {
            panic!("Expected Concurrency");
        }
    }

    #[test]
    fn event_not_found_error_creates_exception() {
        let message = "No events found".to_string();
        let exception = EventStoreError::EventNotFound {
            message: message.clone(),
            query_description: None,
        };
        assert_eq!(exception.to_string(), message);
    }

    #[test]
    fn event_not_found_error_with_query_description_creates_exception() {
        let message = "No events found for aggregate".to_string();
        let query_description = Some("CourseId: 123".to_string());

        let exception = EventStoreError::EventNotFound {
            message: message.clone(),
            query_description: query_description.clone(),
        };

        assert_eq!(exception.to_string(), message);
        if let EventStoreError::EventNotFound {
            query_description: q_desc,
            ..
        } = exception
        {
            assert_eq!(q_desc, query_description);
        } else {
            panic!("Expected EventNotFound");
        }
    }
}
