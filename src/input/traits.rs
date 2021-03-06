use core::convert::Infallible;
use core::ops::Range;

use crate::display::InputDisplay;
#[cfg(feature = "retry")]
use crate::error::ToRetryRequirement;
use crate::error::{
    with_context, ExpectedContext, ExpectedLength, ExpectedValid, ExpectedValue, Length,
    OperationContext, Value, WithContext,
};
use crate::fmt::{Debug, Display, DisplayBase};
use crate::input::pattern::Pattern;
use crate::reader::Reader;
use crate::util::slice;

use super::{Bound, Bytes, MaybeString, Prefix, String};

/// An [`Input`] is an immutable wrapper around bytes to be processed.
///
/// It can only be created via [`dangerous::input()`] as so to clearly point out
/// where untrusted / dangerous input is consumed and takes the form of either
/// [`Bytes`] or [`String`].
///
/// It is used along with [`Reader`] to process the input.
///
/// # Formatting
///
/// `Input` implements support for pretty printing. See [`InputDisplay`] for
/// formatting options.
///
/// [`dangerous::input()`]: crate::input()
#[must_use = "input must be consumed"]
pub trait Input<'i>: Private<'i> {
    /// Returns the [`Input`] [`Bound`].
    fn bound(&self) -> Bound;

    /// Returns `self` as a bound `Input`.
    ///
    /// Bound `Input` carries the guarantee that it will not be extended in
    /// future passes and as a result will not produce [`RetryRequirement`]s.
    ///
    /// # Example
    ///
    /// ```
    /// use dangerous::{Input, Invalid, ToRetryRequirement};
    ///
    /// let error: Invalid = dangerous::input(b"1234")
    ///     .into_bound()
    ///     .read_partial(|r| r.take(5))
    ///     .unwrap_err();
    ///
    /// // If the input was not bound, this wouldn't be fatal.
    /// assert!(error.is_fatal());
    /// ```
    ///
    /// [`RetryRequirement`]: crate::error::RetryRequirement
    fn into_bound(self) -> Self;

    /// Consumes `self` into [`Bytes`].
    fn into_bytes(self) -> Bytes<'i>;

    /// Consumes `self` into [`MaybeString`] returning `MaybeString::String` if
    /// the underlying bytes are known `UTF-8`.
    fn into_maybe_string(self) -> MaybeString<'i>;

    /// Returns an [`InputDisplay`] for formatting.
    fn display(&self) -> InputDisplay<'i>;

    ///////////////////////////////////////////////////////////////////////////
    // Provided methods

    /// Returns the underlying byte slice length.
    #[must_use]
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.as_dangerous_bytes().len()
    }

    /// Returns `true` if the underlying byte slice length is zero.
    #[must_use]
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.byte_len() == 0
    }

    /// Returns `true` if [`Self::bound()`] is [`Bound::Both`].
    #[must_use]
    #[inline(always)]
    fn is_bound(&self) -> bool {
        self.bound() == Bound::Both
    }

    /// Returns `true` if the underlying byte slice for `parent` contains that
    /// of `self` in the same section of memory with no bounds out of range.
    #[must_use]
    fn is_within<'p>(&self, parent: &impl Input<'p>) -> bool {
        slice::is_sub_slice(parent.as_dangerous_bytes(), self.as_dangerous_bytes())
    }

    /// Returns `Some(Range)` with the `start` and `end` offsets of `self`
    /// within the `parent`. `None` is returned if `self` is not within in the
    /// `parent`.
    ///
    /// # Example
    ///
    /// ```
    /// use dangerous::Input;
    ///
    /// let parent = dangerous::input(&[1, 2, 3, 4]);
    /// let sub_range = 1..2;
    /// let sub = dangerous::input(&parent.as_dangerous()[sub_range.clone()]);
    ///
    /// assert_eq!(sub.span_of(&parent), Some(sub_range))
    /// ```
    #[must_use]
    fn span_of<'p>(&self, parent: &impl Input<'p>) -> Option<Range<usize>> {
        if self.is_within(parent) {
            let parent_bounds = parent.as_dangerous_bytes().as_ptr_range();
            let sub_bounds = self.as_dangerous_bytes().as_ptr_range();
            let start_offset = sub_bounds.start as usize - parent_bounds.start as usize;
            let end_offset = sub_bounds.end as usize - parent_bounds.start as usize;
            Some(start_offset..end_offset)
        } else {
            None
        }
    }

    /// Returns `Some(Range)` with the `start` and `end` offsets of `self`
    /// within the `parent`. `None` is returned if `self` is not within in the
    /// `parent` or `self` is empty.
    #[must_use]
    fn span_of_non_empty<'p>(&self, parent: &impl Input<'p>) -> Option<Range<usize>> {
        if self.is_empty() {
            None
        } else {
            self.span_of(parent)
        }
    }

    /// Create a reader with the expectation all of the input is read.
    ///
    /// # Errors
    ///
    /// Returns an error if either the provided function does, or there is
    /// trailing input.
    #[inline]
    fn read_all<F, T, E>(self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut Reader<'i, E, Self>) -> Result<T, E>,
        E: WithContext<'i>,
        E: From<ExpectedLength<'i>>,
    {
        let mut r = Reader::new(self.clone());
        match r.context(OperationContext("read all"), f) {
            Ok(ok) if r.at_end() => Ok(ok),
            Ok(_) => Err(E::from(ExpectedLength {
                len: Length::Exactly(0),
                span: r.take_remaining().as_dangerous_bytes(),
                input: self.into_maybe_string(),
                context: ExpectedContext {
                    operation: "read all",
                    expected: "no trailing input",
                },
            })),
            Err(err) => Err(err),
        }
    }

    /// Create a reader to read a part of the input and return the rest.
    ///
    /// # Errors
    ///
    /// Returns an error if the provided function does.
    #[inline]
    fn read_partial<F, T, E>(self, f: F) -> Result<(T, Self), E>
    where
        F: FnOnce(&mut Reader<'i, E, Self>) -> Result<T, E>,
        E: WithContext<'i>,
    {
        let mut r = Reader::new(self);
        match r.context(OperationContext("read partial"), f) {
            Ok(ok) => Ok((ok, r.take_remaining())),
            Err(err) => Err(err),
        }
    }

    /// Create a reader to read a part of the input and return the rest
    /// without any errors.
    #[inline]
    fn read_infallible<F, T>(self, f: F) -> (T, Self)
    where
        F: FnOnce(&mut Reader<'i, Infallible, Self>) -> T,
    {
        let mut r = Reader::new(self);
        let ok = f(&mut r);
        (ok, r.take_remaining())
    }
}

///////////////////////////////////////////////////////////////////////////////
// Private requirements for any `Input`

pub trait Private<'i>: Sized + Clone + DisplayBase + Debug + Display {
    /// Smallest unit that can be consumed.
    type Token: BytesLength + Copy + 'static;

    /// Iterator of tokens.
    ///
    /// Returns the byte index of the token and the token itself.
    type TokenIter: Iterator<Item = (usize, Self::Token)>;

    /// Returns an empty `Input` pointing the end of `self`.
    fn end(self) -> Self;

    /// Returns a token iterator.
    fn tokens(self) -> Self::TokenIter;

    // Return self with its end bound removed.
    fn into_unbound_end(self) -> Self;

    /// Split the input at the token index `mid`.
    ///
    /// Return `Some` if mid is a valid index, `None` if not.
    fn split_at_opt(self, mid: usize) -> Option<(Self, Self)>;

    /// Splits the input at the byte index `mid` without any validation.
    ///
    /// # Safety
    ///
    /// Caller must guarantee that it is valid to split the structure at `mid`.
    unsafe fn split_at_byte_unchecked(self, mid: usize) -> (Self, Self);
}

///////////////////////////////////////////////////////////////////////////////
// Private extensions to any `Input`

pub(crate) trait PrivateExt<'i>: Input<'i> {
    /// Returns the underlying byte slice of the input.
    #[inline(always)]
    fn as_dangerous_bytes(&self) -> &'i [u8] {
        self.clone().into_bytes().as_dangerous()
    }

    /// Splits the input into two at `mid`.
    ///
    /// # Errors
    ///
    /// Returns an error if `mid > self.len()`.
    #[inline(always)]
    fn split_at<E>(self, mid: usize, operation: &'static str) -> Result<(Self, Self), E>
    where
        E: From<ExpectedLength<'i>>,
    {
        self.clone().split_at_opt(mid).ok_or_else(|| {
            E::from(ExpectedLength {
                len: Length::AtLeast(mid),
                span: self.as_dangerous_bytes(),
                input: self.into_maybe_string(),
                context: ExpectedContext {
                    operation,
                    expected: "enough input",
                },
            })
        })
    }

    /// Splits the input into the first token and whatever remains.
    #[inline(always)]
    fn split_first_opt(self) -> Option<(Self::Token, Self)> {
        self.clone().tokens().next().map(|(_, t)| {
            // SAFETY: ByteLength guarantees a correct implementation for
            // returning the length of a token. The token iterator returned a
            // token for us, so we know we can split it off safely.
            let (_, tail) = unsafe { self.split_at_byte_unchecked(t.byte_len()) };
            (t, tail)
        })
    }

    /// Splits the input into the first token and whatever remains.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is empty.
    #[inline(always)]
    fn split_first<E>(self, operation: &'static str) -> Result<(Self::Token, Self), E>
    where
        E: From<ExpectedLength<'i>>,
    {
        self.clone().split_first_opt().ok_or_else(|| {
            E::from(ExpectedLength {
                len: Length::AtLeast(1),
                span: self.as_dangerous_bytes(),
                input: self.into_maybe_string(),
                context: ExpectedContext {
                    operation,
                    expected: "enough input",
                },
            })
        })
    }

    /// Splits a prefix from the input if it is present.
    #[inline(always)]
    fn split_prefix_opt<P>(self, prefix: P) -> (Option<Self>, Self)
    where
        P: Prefix<Self>,
    {
        if prefix.is_prefix_of(&self) {
            // SAFETY: we just validated that prefix is within the input so its
            // length is a valid index.
            let (head, tail) = unsafe { self.split_at_byte_unchecked(prefix.byte_len()) };
            (Some(head), tail)
        } else {
            (None, self)
        }
    }

    /// Splits a prefix from the input if it is present.
    ///
    /// # Errors
    ///
    /// Returns an error if the input does not have the prefix.
    #[inline(always)]
    fn split_prefix<P, E>(self, prefix: P, operation: &'static str) -> Result<(Self, Self), E>
    where
        E: From<ExpectedValue<'i>>,
        P: Prefix<Self> + Into<Value<'i>>,
    {
        match self.clone().split_prefix_opt(&prefix) {
            (Some(head), tail) => Ok((head, tail)),
            (None, unmatched) => {
                let bytes = unmatched.as_dangerous_bytes();
                let prefix_len = prefix.byte_len();
                let actual = if bytes.len() > prefix_len {
                    &bytes[..prefix_len]
                } else {
                    &bytes
                };
                Err(E::from(ExpectedValue {
                    actual,
                    expected: prefix.into(),
                    input: self.into_maybe_string(),
                    context: ExpectedContext {
                        operation,
                        expected: "exact value",
                    },
                }))
            }
        }
    }

    /// Splits at a pattern in the input if it is present.
    #[inline(always)]
    fn split_until_opt<P>(self, pattern: P) -> Option<(Self, Self)>
    where
        P: Pattern<Self>,
    {
        pattern.find_match(&self).map(|(index, _)| {
            // SAFETY: Pattern guarantees it returns valid indexes.
            unsafe { self.split_at_byte_unchecked(index) }
        })
    }

    /// Splits at a pattern in the input if it is present.
    #[inline(always)]
    fn split_until_consume_opt<P>(self, pattern: P) -> Option<(Self, Self)>
    where
        P: Pattern<Self>,
    {
        pattern.find_match(&self).map(|(index, len)| {
            // SAFETY: Pattern guarantees it returns valid indexes.
            let (head, tail) = unsafe { self.split_at_byte_unchecked(index) };
            let (_, tail) = unsafe { tail.split_at_byte_unchecked(len) };
            (head, tail)
        })
    }

    /// Splits the input up to when the pattern matches.
    #[inline(always)]
    fn split_until<P, E>(self, pattern: P, operation: &'static str) -> Result<(Self, Self), E>
    where
        E: From<ExpectedValue<'i>>,
        P: Pattern<Self> + Into<Value<'i>> + Copy,
    {
        self.clone().split_until_opt(pattern).ok_or_else(|| {
            E::from(ExpectedValue {
                actual: self.as_dangerous_bytes(),
                expected: pattern.into(),
                input: self.into_maybe_string(),
                context: ExpectedContext {
                    operation,
                    expected: "pattern match",
                },
            })
        })
    }

    /// Splits at a pattern in the input if it is present.
    ///
    /// # Errors
    ///
    /// Returns an error if the input does not have the pattern.
    #[inline(always)]
    fn split_until_consume<P, E>(
        self,
        pattern: P,
        operation: &'static str,
    ) -> Result<(Self, Self), E>
    where
        E: From<ExpectedValue<'i>>,
        P: Pattern<Self> + Into<Value<'i>> + Copy,
    {
        self.clone()
            .split_until_consume_opt(pattern)
            .ok_or_else(|| {
                E::from(ExpectedValue {
                    actual: self.as_dangerous_bytes(),
                    expected: pattern.into(),
                    input: self.into_maybe_string(),
                    context: ExpectedContext {
                        operation,
                        expected: "pattern match",
                    },
                })
            })
    }

    /// Splits the input up to when the provided function returns `false`.
    #[inline(always)]
    fn split_while_opt<P>(self, pattern: P) -> Option<(Self, Self)>
    where
        P: Pattern<Self>,
    {
        pattern.find_reject(&self).map(|i| {
            // SAFETY: Pattern guarantees it returns valid indexes.
            unsafe { self.split_at_byte_unchecked(i) }
        })
    }

    /// Tries to split the input up to when the provided function returns
    /// `false`.
    ///
    /// # Errors
    ///
    /// Returns an error from the provided function if it fails.
    #[inline(always)]
    fn try_split_while<F, E>(self, mut f: F, operation: &'static str) -> Result<(Self, Self), E>
    where
        E: WithContext<'i>,
        F: FnMut(Self::Token) -> Result<bool, E>,
    {
        // For each token, lets make sure it matches the predicate.
        for (i, token) in self.clone().tokens() {
            // Check if the token doesn't match the predicate.
            if !with_context(self.clone(), OperationContext(operation), || f(token))? {
                // Split the input up to, but not including the token.
                // `i` derived from the token iterator is always a valid index
                // for the input.
                let (head, tail) = unsafe { self.split_at_byte_unchecked(i) };
                // Return the split input parts.
                return Ok((head, tail));
            }
        }
        Ok((self.clone(), self.end()))
    }

    /// Splits the input at what was read and what was remaining.
    #[inline(always)]
    fn split_consumed<F, E>(self, f: F) -> (Self, Self)
    where
        F: FnOnce(&mut Reader<'i, E, Self>),
    {
        let mut reader = Reader::new(self.clone());
        // Consume input.
        f(&mut reader);
        // We take the remaining input.
        let tail = reader.take_remaining();
        // For the head, we take what we consumed.
        let mid = self.byte_len() - tail.byte_len();
        // SAFETY: we take mid as the difference between the parent slice and
        // the remaining slice left over from the reader. This means the index
        // can only ever be valid.
        let (head, _) = unsafe { self.split_at_byte_unchecked(mid) };
        // We derive the bound constraint from self. If the tail start is
        // undetermined this means the last bit of input consumed could be
        // longer if there was more available and as such makes the end of input
        // we return unbounded.
        if tail.bound() == Bound::None {
            (head.into_unbound_end(), tail)
        } else {
            (head, tail)
        }
    }

    /// Tries to split the input at what was read and what was remaining.
    ///
    /// # Errors
    ///
    /// Returns an error from the provided function if it fails.
    #[inline(always)]
    fn try_split_consumed<F, E>(self, f: F, operation: &'static str) -> Result<(Self, Self), E>
    where
        E: WithContext<'i>,
        F: FnOnce(&mut Reader<'i, E, Self>) -> Result<(), E>,
    {
        let mut reader = Reader::new(self.clone());
        // Consume input.
        reader.context(OperationContext(operation), f)?;
        // We take the remaining input.
        let tail = reader.take_remaining();
        // For the head, we take what we consumed.
        let mid = self.byte_len() - tail.byte_len();
        // SAFETY: we take mid as the difference between the parent slice and
        // the remaining slice left over from the reader. This means the index
        // can only ever be valid.
        let (head, _) = unsafe { self.split_at_byte_unchecked(mid) };
        // We derive the bound constraint from self. If the tail start is
        // undetermined this means the last bit of input consumed could be
        // longer if there was more available and as such makes the end of input
        // we return unbounded.
        if tail.bound() == Bound::None {
            Ok((head.into_unbound_end(), tail))
        } else {
            Ok((head, tail))
        }
    }

    /// Splits the input from the value expected to be read.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected value was not present.
    #[inline(always)]
    fn split_expect<F, T, E>(
        self,
        f: F,
        expected: &'static str,
        operation: &'static str,
    ) -> Result<(T, Self), E>
    where
        E: From<ExpectedValid<'i>>,
        F: FnOnce(&mut Reader<'i, E, Self>) -> Option<T>,
    {
        let context = ExpectedContext {
            expected,
            operation,
        };
        let mut reader = Reader::new(self.clone());
        if let Some(ok) = f(&mut reader) {
            Ok((ok, reader.take_remaining()))
        } else {
            let tail = reader.take_remaining();
            let span = &self.as_dangerous_bytes()[..self.byte_len() - tail.byte_len()];
            Err(E::from(ExpectedValid {
                span,
                input: self.into_maybe_string(),
                context,
                #[cfg(feature = "retry")]
                retry_requirement: None,
            }))
        }
    }

    /// Tries to split the input from the value expected to be read.
    ///
    /// # Errors
    ///
    /// Returns an error from the provided function if it fails or if the
    /// expected value was not present.
    #[inline(always)]
    fn try_split_expect<F, T, E>(
        self,
        f: F,
        expected: &'static str,
        operation: &'static str,
    ) -> Result<(T, Self), E>
    where
        E: WithContext<'i>,
        E: From<ExpectedValid<'i>>,
        F: FnOnce(&mut Reader<'i, E, Self>) -> Result<Option<T>, E>,
    {
        let context = ExpectedContext {
            expected,
            operation,
        };
        let mut reader = Reader::new(self.clone());
        match reader.context(context, f) {
            Ok(Some(ok)) => Ok((ok, reader.take_remaining())),
            Ok(None) => {
                let tail = reader.take_remaining();
                let span = &self.as_dangerous_bytes()[..self.byte_len() - tail.byte_len()];
                Err(E::from(ExpectedValid {
                    span,
                    input: self.into_maybe_string(),
                    context,
                    #[cfg(feature = "retry")]
                    retry_requirement: None,
                }))
            }
            Err(err) => Err(err),
        }
    }

    /// Tries to split the input from the value expected to be read.
    ///
    /// # Errors
    ///
    /// Returns an erased error from the provided function if it fails or if the
    /// expected value was not present.
    #[cfg(feature = "retry")]
    #[inline(always)]
    fn try_split_expect_erased<F, T, R, E>(
        self,
        f: F,
        expected: &'static str,
        operation: &'static str,
    ) -> Result<(T, Self), E>
    where
        E: WithContext<'i>,
        E: From<ExpectedValid<'i>>,
        F: FnOnce(&mut Reader<'i, E, Self>) -> Result<T, R>,
        R: ToRetryRequirement,
    {
        let mut reader = Reader::new(self.clone());
        match f(&mut reader) {
            Ok(ok) => Ok((ok, reader.take_remaining())),
            Err(err) => {
                let tail = reader.take_remaining();
                let span = &self.as_dangerous_bytes()[..self.byte_len() - tail.byte_len()];
                Err(E::from(ExpectedValid {
                    span,
                    input: self.into_maybe_string(),
                    context: ExpectedContext {
                        expected,
                        operation,
                    },
                    retry_requirement: err.to_retry_requirement(),
                }))
            }
        }
    }
}

impl<'i, T> PrivateExt<'i> for T where T: Input<'i> {}

///////////////////////////////////////////////////////////////////////////////
// BytesLength

pub unsafe trait BytesLength: Copy {
    fn byte_len(self) -> usize;
}

unsafe impl<T> BytesLength for &T
where
    T: BytesLength,
{
    #[inline(always)]
    fn byte_len(self) -> usize {
        (*self).byte_len()
    }
}

unsafe impl BytesLength for u8 {
    #[inline(always)]
    fn byte_len(self) -> usize {
        1
    }
}

unsafe impl BytesLength for char {
    #[inline(always)]
    fn byte_len(self) -> usize {
        self.len_utf8()
    }
}

unsafe impl BytesLength for &[u8] {
    #[inline(always)]
    fn byte_len(self) -> usize {
        self.len()
    }
}

unsafe impl BytesLength for &str {
    #[inline(always)]
    fn byte_len(self) -> usize {
        self.as_bytes().len()
    }
}

macro_rules! impl_array_bytes_len {
    ($($n:expr),*) => {
        $(
            unsafe impl BytesLength for &[u8; $n] {
                #[inline(always)]
                fn byte_len(self) -> usize {
                    self.len()
                }
            }
        )*
    };
}

for_common_array_sizes!(impl_array_bytes_len);

///////////////////////////////////////////////////////////////////////////////
// IntoInput

pub trait IntoInput<'i>: Copy {
    type Input: Input<'i>;

    fn into_input(self) -> Self::Input;
}

impl<'i, T> IntoInput<'i> for &T
where
    T: IntoInput<'i>,
{
    type Input = T::Input;

    #[inline(always)]
    fn into_input(self) -> Self::Input {
        (*self).into_input()
    }
}

impl<'i> IntoInput<'i> for &'i [u8] {
    type Input = Bytes<'i>;

    #[inline(always)]
    fn into_input(self) -> Self::Input {
        Bytes::new(self, Bound::Start)
    }
}

impl<'i> IntoInput<'i> for &'i str {
    type Input = String<'i>;

    #[inline(always)]
    fn into_input(self) -> Self::Input {
        String::new(self, Bound::Start)
    }
}

macro_rules! impl_array_into_input {
    ($($n:expr),*) => {
        $(
            impl<'i> IntoInput<'i> for &'i [u8; $n] {
                type Input = Bytes<'i>;

                #[inline(always)]
                fn into_input(self) -> Self::Input {
                    Bytes::new(self, Bound::Start)
                }
            }
        )*
    };
}

for_common_array_sizes!(impl_array_into_input);
