use std::borrow::Cow;

use crate::error::SeaboredDeError;

pub type ReadResult<'data, T> = Result<T, SeaboredDeError<'data>>;

pub trait Read<'data> {
    /// Peek a single byte over the data stream, without advancing it.
    /// This is mostly used for looking ahead at the CBOR Initial Byte
    /// and taking decisions over it
    fn peek_byte(&mut self) -> ReadResult<'data, u8>;
    /// Manually advances the data stream by `n` bytes. Usually used in tandem with [`Self::peek_byte`]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()>;
    /// Read a single byte from the data stream
    fn read_byte(&mut self) -> ReadResult<'data, u8>;
    /// Read a byte slice from the data stream of length `len`
    /// Errors out if `n` is bigger than the remaining data stream
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>>;
    /// Read a byte array from the data stream of len `N`
    /// Basically behaves the same as [`Self::read_slice`] but with a sized slice
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>>;

    /// Provided implementation that hopefully erases the internal Cow for performance
    #[inline]
    fn read_be_u16(&mut self) -> ReadResult<'data, u16> {
        self.read_array().map(|arr| u16::from_be_bytes(*arr))
    }

    /// Provided implementation that hopefully erases the internal Cow for performance
    #[inline]
    fn read_be_u32(&mut self) -> ReadResult<'data, u32> {
        self.read_array().map(|arr| u32::from_be_bytes(*arr))
    }

    /// Provided implementation that hopefully erases the internal Cow for performance
    #[inline]
    fn read_be_u64(&mut self) -> ReadResult<'data, u64> {
        self.read_array().map(|arr| u64::from_be_bytes(*arr))
    }
}

// Blanket impls
impl<'data, R: Read<'data> + ?Sized> Read<'data> for &mut R {
    #[inline(always)]
    fn peek_byte(&mut self) -> ReadResult<'data, u8> {
        (**self).peek_byte()
    }

    #[inline(always)]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()> {
        (**self).advance(n)
    }

    #[inline(always)]
    fn read_byte(&mut self) -> ReadResult<'data, u8> {
        (**self).read_byte()
    }

    #[inline(always)]
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>> {
        (**self).read_slice(len)
    }

    #[inline(always)]
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>> {
        (**self).read_array()
    }
}

impl<'data, R: Read<'data> + ?Sized> Read<'data> for Box<R> {
    #[inline(always)]
    fn peek_byte(&mut self) -> ReadResult<'data, u8> {
        (**self).peek_byte()
    }

    #[inline(always)]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()> {
        (**self).advance(n)
    }

    #[inline(always)]
    fn read_byte(&mut self) -> ReadResult<'data, u8> {
        (**self).read_byte()
    }

    #[inline(always)]
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>> {
        (**self).read_slice(len)
    }

    #[inline(always)]
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>> {
        (**self).read_array()
    }
}

#[derive(Debug)]
/// Depth-aware wrapper around a [`Read`] implementation.
/// If you care about billion-laughs attacks, then you should use it.
pub struct DepthAwareReader<'data, R: Read<'data>> {
    reader: R,
    limit: usize,
    depth: usize,
    _marker: std::marker::PhantomData<&'data ()>,
}

impl<'data, R: Read<'data>> DepthAwareReader<'data, R> {
    /// Default depth limit
    pub const DEFAULT_LIMIT: usize = 256;

    /// Initializes a DepthAwareReader using [`Self::DEFAULT_LIMIT`] as a depth limit
    #[inline(always)]
    pub fn from_reader(reader: R) -> Self {
        Self {
            reader,
            limit: Self::DEFAULT_LIMIT,
            depth: 0,
            _marker: Default::default(),
        }
    }

    /// Initializes a DepthAwareReader using a custom depth limit
    #[inline(always)]
    pub fn from_reader_with_limit(reader: R, limit: usize) -> Self {
        Self {
            reader,
            limit,
            depth: 0,
            _marker: Default::default(),
        }
    }

    /// Increments the current depth, checks if it didn't exceed the limit and
    /// returns a Guard that exits this depth when dropped
    #[inline(always)]
    pub fn enter(&mut self) -> Result<DepthAwareReaderGuard<'_, 'data, R>, SeaboredDeError<'data>> {
        self.depth += 1;
        if self.depth <= self.limit {
            Ok(DepthAwareReaderGuard::new(self))
        } else {
            Err(SeaboredDeError::AllowedDepthOverflow {
                depth: self.depth,
                limit: self.limit,
            })
        }
    }
}

/// Guard that decredements the current depth when dropped
pub struct DepthAwareReaderGuard<'a, 'data, R: Read<'data>> {
    rdr: &'a mut DepthAwareReader<'data, R>,
    accumulated_depth: usize,
}

impl<'a, 'data, R: Read<'data>> DepthAwareReaderGuard<'a, 'data, R> {
    fn new(rdr: &'a mut DepthAwareReader<'data, R>) -> Self {
        Self {
            rdr,
            accumulated_depth: 1,
        }
    }

    pub fn enter(mut self) -> Result<Self, SeaboredDeError<'data>> {
        self.rdr.depth += 1;
        self.accumulated_depth += 1;
        if self.rdr.depth <= self.rdr.limit {
            Ok(self)
        } else {
            Err(SeaboredDeError::AllowedDepthOverflow {
                depth: self.rdr.depth,
                limit: self.rdr.limit,
            })
        }
    }
}

impl<'a, 'data, R: Read<'data>> Drop for DepthAwareReaderGuard<'a, 'data, R> {
    #[inline(always)]
    fn drop(&mut self) {
        self.rdr.depth -= self.accumulated_depth;
    }
}

/// Delegate reader impl
impl<'data, R: Read<'data>> Read<'data> for DepthAwareReader<'data, R> {
    #[inline(always)]
    fn peek_byte(&mut self) -> ReadResult<'data, u8> {
        self.reader.peek_byte()
    }

    #[inline(always)]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()> {
        self.reader.advance(n)
    }

    #[inline(always)]
    fn read_byte(&mut self) -> ReadResult<'data, u8> {
        self.reader.read_byte()
    }

    #[inline(always)]
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>> {
        self.reader.read_slice(len)
    }

    #[inline(always)]
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>> {
        self.reader.read_array()
    }
}

/// Implementation on base slices. This is what you'd want to use when having something that can hold
/// in-memory or when memory-mapping large-ish files
///
/// ## Attention
/// - Careful, Windows isn't very happy with 4GB memory-maps because 32-bit mmap is so 1980.
impl<'data> Read<'data> for &'data [u8] {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn peek_byte(&mut self) -> ReadResult<'data, u8> {
        if self.is_empty() {
            return Err(SeaboredDeError::IoKind(std::io::ErrorKind::UnexpectedEof));
        }
        // SAFETY: The precondition above satisfies invariants
        Ok(unsafe { *self.as_ptr() })
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()> {
        if n > self.len() {
            return Err(SeaboredDeError::IoKind(std::io::ErrorKind::UnexpectedEof));
        }

        // SAFETY: The check above fulfill the invariants required by this function
        *self = unsafe { std::slice::from_raw_parts(self.as_ptr().add(n), self.len() - n) };
        Ok(())
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn read_byte(&mut self) -> ReadResult<'data, u8> {
        if self.is_empty() {
            return Err(SeaboredDeError::IoKind(std::io::ErrorKind::UnexpectedEof));
        }
        let ptr = self.as_ptr();
        // SAFETY: The precondition above satisfies invariants of dereferencing the buffer pointer
        // (it dereferences to the first element of the slice, which is a byte)
        let b = unsafe { *ptr };
        // SAFETY: The above call will error out if the preconditions for from_raw_parts wouldn't be fulfilled
        *self = unsafe { std::slice::from_raw_parts(ptr.add(1), self.len() - 1) };
        Ok(b)
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>> {
        let Some((start, end)) = self.split_at_checked(len) else {
            return Err(SeaboredDeError::IoKind(std::io::ErrorKind::UnexpectedEof));
        };

        *self = end;
        Ok(Cow::Borrowed(start))
    }

    #[inline]
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>> {
        let Some((start, end)) = self.split_at_checked(N) else {
            return Err(SeaboredDeError::IoKind(std::io::ErrorKind::UnexpectedEof));
        };

        *self = end;
        // SAFETY: The `start` slice will always be exactly N elements long, so this is safe.
        // Anyway this is basically what `slice::as_array` is, but without the length checks
        // as they are done above in `split_at_checked`.
        //
        // From std:
        // > SAFETY: The underlying array of a slice can be reinterpreted as an actual
        // > array `[T; N]` if `N` is not greater than the slice's length.
        Ok(Cow::Borrowed(unsafe { &*start.as_ptr().cast() }))
    }
}

#[derive(Debug)]
#[repr(transparent)]
/// Wrapper around any [`std::io::Read`] type, necessary not to conflict with the implementation on [`&[u8]`]
///
/// ## Warning
///
/// This is not zero-copy!
pub struct StdReader<T: std::io::Read + std::io::Seek>(T);

impl<T: std::io::Read + std::io::Seek> StdReader<T> {
    #[inline(always)]
    pub fn new(reader: T) -> Self {
        Self::from(reader)
    }

    #[inline(always)]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: std::io::Read + std::io::Seek> From<T> for StdReader<T> {
    #[inline(always)]
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<'data, T: std::io::Read + std::io::Seek> Read<'data> for StdReader<T> {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn peek_byte(&mut self) -> ReadResult<'data, u8> {
        let b = self.read_byte()?;
        // Walk back!
        self.0.seek_relative(-1)?;
        Ok(b)
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn advance(&mut self, n: usize) -> ReadResult<'data, ()> {
        self.0.seek_relative(n as i64)?;
        Ok(())
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn read_byte(&mut self) -> ReadResult<'data, u8> {
        let mut b = 0;
        self.0.read_exact(std::slice::from_mut(&mut b))?;
        Ok(b)
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn read_slice<'a>(&'a mut self, len: usize) -> ReadResult<'data, Cow<'data, [u8]>> {
        let mut buf = vec![0; len];
        self.0.read_exact(&mut buf)?;
        Ok(Cow::Owned(buf))
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn read_array<const N: usize>(&mut self) -> ReadResult<'data, Cow<'data, [u8; N]>> {
        let mut arr = [0; N];
        self.0.read_exact(&mut arr)?;
        Ok(Cow::Owned(arr))
    }
}

impl<T: std::io::Read + std::io::Seek> std::io::Read for StdReader<T> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl<T: std::io::Read + std::io::Seek> std::io::Seek for StdReader<T> {
    #[inline(always)]
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl<T: std::io::BufRead + std::io::Seek> std::io::BufRead for StdReader<T> {
    #[inline(always)]
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.0.fill_buf()
    }

    #[inline(always)]
    fn consume(&mut self, amount: usize) {
        self.0.consume(amount)
    }
}
