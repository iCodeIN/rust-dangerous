use core::fmt;

use crate::input::Input;

use super::{
    Context, ExpectedLength, ExpectedValid, ExpectedValue, FromContext, RetryRequirement,
    ToRetryRequirement,
};

/// `Fatal` contains no details around what went wrong and cannot be retried.
///
/// This is the most performant and simplistic catch-all error, but it doesn't
/// provide any context to debug problems well and cannot be used in streaming
/// contexts.
///
/// See [`crate::error`] for additional documentation around the error system.
///
/// # Example
///
/// ```
/// use dangerous::Fatal;
///
/// let error: Fatal = dangerous::input(b"").read_all(|r| {
///     r.read_u8()
/// }).unwrap_err();
///
/// assert_eq!(
///     error.to_string(),
///     "invalid input",
/// );
/// ```
#[derive(PartialEq)]
pub struct Fatal;

impl fmt::Debug for Fatal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Fatal").finish()
    }
}

impl fmt::Display for Fatal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid input")
    }
}

impl<'i> FromContext<'i> for Fatal {
    fn from_context<C>(self, _input: Input<'i>, _context: C) -> Self
    where
        C: Context,
    {
        self
    }
}

impl ToRetryRequirement for Fatal {
    fn to_retry_requirement(&self) -> Option<RetryRequirement> {
        None
    }

    fn is_fatal(&self) -> bool {
        true
    }
}

impl<'i> From<ExpectedValue<'i>> for Fatal {
    fn from(_: ExpectedValue<'i>) -> Self {
        Self
    }
}

impl<'i> From<ExpectedLength<'i>> for Fatal {
    fn from(_: ExpectedLength<'i>) -> Self {
        Self
    }
}

impl<'i> From<ExpectedValid<'i>> for Fatal {
    fn from(_: ExpectedValid<'i>) -> Self {
        Self
    }
}
