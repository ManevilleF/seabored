use crate::error::SeaboredSerError;

pub trait Write {
    /// Writes a buffer to the output.
    /// There's no guarantee that *everything* in the buffer is written.
    /// If you require such a guarantee, use the [`Self::write_all`] function
    ///
    /// Returns the number of bytes written
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError>;
    /// Flushes the buffer to whatever underlying I/O exists,
    /// it can be network, a file, or whatever.
    /// For example, for Files, this will be where a call to `fsync(1)` will be done
    fn flush(&mut self) -> Result<(), SeaboredSerError>;

    /// Writes the whole buffer to the underlying storage
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write_all(&mut self, mut buf: &[u8]) -> Result<(), SeaboredSerError> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => {
                    return Err(SeaboredSerError::IoKind(std::io::ErrorKind::WriteZero));
                }
                // Advance the pointer for next write
                Ok(n) => buf = &buf[n..],
                Err(SeaboredSerError::Io(ref error))
                    if error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    continue;
                }
                Err(SeaboredSerError::IoKind(std::io::ErrorKind::Interrupted)) => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    /// Vectorized version of writes
    /// This can yield significant speedups for backing I/O that has vectorization possible
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError>;
}

// Blanket impls
impl<W: Write + ?Sized> Write for &mut W {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError> {
        (**self).write(buf)
    }

    #[inline(always)]
    fn flush(&mut self) -> Result<(), SeaboredSerError> {
        (**self).flush()
    }

    #[inline(always)]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), SeaboredSerError> {
        (**self).write_all(buf)
    }

    #[inline(always)]
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError> {
        (**self).write_vectored(bufs)
    }
}

impl<W: Write + ?Sized> Write for Box<W> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError> {
        (**self).write(buf)
    }

    #[inline(always)]
    fn flush(&mut self) -> Result<(), SeaboredSerError> {
        (**self).flush()
    }

    #[inline(always)]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), SeaboredSerError> {
        (**self).write_all(buf)
    }

    #[inline(always)]
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError> {
        (**self).write_vectored(bufs)
    }
}

impl Write for &mut [u8] {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError> {
        let amt = buf.len().min(self.len());
        let (tbw, end) = std::mem::take(self).split_at_mut(amt);
        tbw.copy_from_slice(&buf[..amt]);
        *self = end;
        Ok(amt)
    }

    #[inline(always)]
    fn flush(&mut self) -> Result<(), SeaboredSerError> {
        Ok(())
    }

    // This is overridden because we don't need a loop; it can always be one-shot unless
    // params are seriously wrong
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), SeaboredSerError> {
        let expected_len = buf.len();
        (self.write(buf)? == expected_len)
            .then_some(())
            .ok_or(SeaboredSerError::IoKind(std::io::ErrorKind::WriteZero))
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError> {
        let mut written = 0;
        for buf in bufs {
            written += self.write(buf)?;
            if self.is_empty() {
                break;
            }
        }
        Ok(written)
    }
}

impl Write for Vec<u8> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError> {
        self.extend(buf);
        Ok(buf.len())
    }

    #[inline(always)]
    fn flush(&mut self) -> Result<(), SeaboredSerError> {
        Ok(())
    }

    #[inline(always)]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), SeaboredSerError> {
        self.write(buf)?;
        Ok(())
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError> {
        let len = bufs.iter().map(|b| b.len()).sum();
        self.reserve(len);
        for buf in bufs {
            self.extend(*buf);
        }
        Ok(len)
    }
}

#[derive(Debug)]
#[repr(transparent)]
/// Wrapper around any [`std::io::Write`] implementer
pub struct StdWriter<W: std::io::Write>(W);

impl<W: std::io::Write> StdWriter<W> {
    #[inline(always)]
    pub fn new(writer: W) -> Self {
        Self::from(writer)
    }

    #[inline(always)]
    pub fn into_inner(self) -> W {
        self.0
    }
}

impl<W: std::io::Write> From<W> for StdWriter<W> {
    #[inline(always)]
    fn from(value: W) -> Self {
        Self(value)
    }
}

impl<W: std::io::Write> Write for StdWriter<W> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, SeaboredSerError> {
        Ok(self.0.write(buf)?)
    }

    #[inline(always)]
    fn flush(&mut self) -> Result<(), SeaboredSerError> {
        Ok(self.0.flush()?)
    }

    #[inline(always)]
    fn write_all(&mut self, buf: &[u8]) -> Result<(), SeaboredSerError> {
        Ok(self.0.write_all(buf)?)
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn write_vectored(&mut self, bufs: &[&[u8]]) -> Result<usize, SeaboredSerError> {
        let bufs = bufs
            .iter()
            .map(|s| std::io::IoSlice::new(s))
            .collect::<Vec<_>>();
        Ok(self.0.write_vectored(&bufs)?)
    }
}

impl<W: std::io::Write> std::io::Write for StdWriter<W> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    #[inline(always)]
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }

    #[inline(always)]
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.0.write_vectored(bufs)
    }

    #[inline(always)]
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.0.write_all(buf)
    }

    #[inline(always)]
    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        self.0.write_fmt(args)
    }
}
