use crate::{
    de::CborDeserialize,
    error::{SeaboredDeError, SeaboredSerError},
    ib::{self, InitialByte},
    io::{Read, Write},
    mt::MajorType,
    ser::CborSerialize,
};

pub(crate) const IB_LIMIT: u8 = 0x17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
#[allow(dead_code)]
pub(crate) enum CborIntegerSize {
    MergeIntoIB = 0,
    U8 = 1,
    U16 = 1 << 1,
    U32 = 1 << 2,
    U64 = 1 << 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
/// A unified CBOR integer that has a u64 representation
pub struct CborIntegerValue(u64);

macro_rules! impl_int_conversions_cbor_int_value {
    (uint $repr:ty) => {
        impl From<$repr> for CborIntegerValue {
            #[inline(always)]
            fn from(value: $repr) -> Self {
                Self(value as u64)
            }
        }

        impl TryInto<$repr> for CborIntegerValue {
            type Error = std::num::TryFromIntError;
            #[inline(always)]
            fn try_into(self) -> Result<$repr, Self::Error> {
                self.0.try_into()
            }
        }
    };
    (infaillible uint $repr:ty ) => {
        impl From<$repr> for CborIntegerValue {
            #[inline(always)]
            fn from(value: $repr) -> Self {
                Self(value)
            }
        }

        impl Into<$repr> for CborIntegerValue {
            #[inline(always)]
            fn into(self) -> $repr {
                self.0
            }
        }
    };
}

impl_int_conversions_cbor_int_value!(uint u8);
impl_int_conversions_cbor_int_value!(uint u16);
impl_int_conversions_cbor_int_value!(uint u32);
impl_int_conversions_cbor_int_value!(infaillible uint u64);

impl From<usize> for CborIntegerValue {
    #[inline(always)]
    fn from(value: usize) -> Self {
        Self(value as u64)
    }
}

impl TryInto<usize> for CborIntegerValue {
    type Error = std::num::TryFromIntError;
    #[inline(always)]
    fn try_into(self) -> Result<usize, Self::Error> {
        self.0.try_into()
    }
}

impl CborIntegerValue {
    #[inline(always)]
    /// Returns a [`CborIntegerSize`] and the InitialByte offset to apply
    pub(crate) fn size(&self) -> (CborIntegerSize, u8) {
        if self.0 <= IB_LIMIT as u64 {
            (CborIntegerSize::MergeIntoIB, 0)
        } else if self.0 <= u8::MAX as u64 {
            (CborIntegerSize::U8, IB_LIMIT + 1)
        } else if self.0 <= u16::MAX as u64 {
            (CborIntegerSize::U16, IB_LIMIT + 2)
        } else if self.0 <= u32::MAX as u64 {
            (CborIntegerSize::U32, IB_LIMIT + 3)
        } else {
            (CborIntegerSize::U64, IB_LIMIT + 4)
        }
    }

    /// Optimized version of [`Self::serialize_complex_mt_preamble`] that does vectored writes
    /// of both the preamble and the bytes after
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    pub(crate) fn serialize_inline_bytes<W: Write>(
        bytes: &[u8],
        mt: MajorType,
        writer: &mut W,
    ) -> Result<usize, SeaboredSerError> {
        debug_assert!(
            matches!(mt, MajorType::Bytes | MajorType::String),
            "[IMPLEMENTATION ERROR] MajorType used must be Bytes or String"
        );

        let len = CborIntegerValue::from(bytes.len());
        let (size, ib_offset) = len.size();

        let ib = InitialByte::from(mt);

        Ok(match size {
            CborIntegerSize::MergeIntoIB => {
                writer.write_vectored(&[&[ib.0 + len.0 as u8], bytes])?
            }
            _ => writer.write_vectored(&[
                &[ib.0 + ib_offset],
                &len.0.to_be_bytes()[(8 - size as u8) as usize..],
                bytes,
            ])?,
        })
    }

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    pub(crate) fn serialize_complex_mt_preamble<W: Write>(
        &self,
        mt: MajorType,
        writer: &mut W,
    ) -> Result<usize, SeaboredSerError> {
        debug_assert!(
            matches!(
                mt,
                MajorType::Bytes
                    | MajorType::String
                    | MajorType::Array
                    | MajorType::Tagged
                    | MajorType::Map
            ),
            "MajorType must be a complex type, including: Bytes, String, Array, Map or Tagged"
        );

        let (size, ib_offset) = self.size();
        let ib = InitialByte::from(mt);

        Ok(match size {
            CborIntegerSize::MergeIntoIB => writer.write(&[ib.0 + self.0 as u8])?,
            _ => writer.write_vectored(&[
                &[ib.0 + ib_offset],
                &self.0.to_be_bytes()[(8 - size as u8) as usize..],
            ])?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CborInteger {
    pub(crate) value: CborIntegerValue,
    pub(crate) negative: bool,
}

impl CborInteger {
    #[inline(always)]
    fn base_ib(&self) -> u8 {
        (self.negative as u8) << 5u8
    }
}

impl CborSerialize for CborInteger {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        let (size, ib_offset) = self.value.size();
        let ib = self.base_ib();

        Ok(match size {
            CborIntegerSize::MergeIntoIB => writer.write(&[ib + self.value.0 as u8])?,
            _ => writer.write_vectored(&[
                &[ib + ib_offset],
                &self.value.0.to_be_bytes()[8 - size as u8 as usize..],
            ])?,
        })
    }
}

impl<'a> CborDeserialize<'a> for CborInteger {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(
        reader: &mut R,
    ) -> Result<Self, crate::error::SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();

        let negative = match mt {
            MajorType::Uint => false,
            MajorType::NegativeUint => true,
            _ => {
                return Err(SeaboredDeError::IncorrectMajorType {
                    actual: mt,
                    expected: &[MajorType::Uint, MajorType::NegativeUint],
                });
            }
        };

        let value = ai.find_subsequent_len(reader)?;

        Ok(Self { value, negative })
    }
}

macro_rules! impl_int_conversion_cbor_int {
    (uint $repr:ty) => {
        impl From<$repr> for CborInteger {
            #[inline(always)]
            fn from(value: $repr) -> Self {
                Self {
                    value: value.into(),
                    negative: false,
                }
            }
        }

        impl TryInto<$repr> for CborInteger {
            type Error = std::num::TryFromIntError;
            #[inline(always)]
            fn try_into(self) -> Result<$repr, Self::Error> {
                self.value.try_into()
            }
        }

        impl TryInto<$repr> for &CborInteger {
            type Error = std::num::TryFromIntError;
            #[inline(always)]
            fn try_into(self) -> Result<$repr, Self::Error> {
                self.value.try_into()
            }
        }
    };

    (infaillible uint $repr:ty ) => {
        impl From<$repr> for CborInteger {
            #[inline(always)]
            fn from(value: $repr) -> Self {
                Self {
                    value: value.into(),
                    negative: false,
                }
            }
        }

        impl Into<$repr> for CborInteger {
            #[inline(always)]
            fn into(self) -> $repr {
                self.value.into()
            }
        }

        impl Into<$repr> for &CborInteger {
            #[inline(always)]
            fn into(self) -> $repr {
                self.value.into()
            }
        }
    };

    (int $repr:ty) => {
        impl From<$repr> for CborInteger {
            #[cfg_attr(feature = "inline-nontrivial", inline)]
            fn from(value: $repr) -> Self {
                let negative = value.is_negative();
                Self {
                    negative,
                    value: value.unsigned_abs().saturating_sub(negative as _).into(),
                }
            }
        }

        impl TryInto<$repr> for CborInteger {
            type Error = std::num::TryFromIntError;
            #[cfg_attr(feature = "inline-nontrivial", inline)]
            fn try_into(self) -> Result<$repr, Self::Error> {
                let value: $repr = self.value.0.try_into()?;
                Ok(-(self.negative as $repr) ^ value)
            }
        }

        impl TryInto<$repr> for &CborInteger {
            type Error = std::num::TryFromIntError;
            #[cfg_attr(feature = "inline-nontrivial", inline)]
            fn try_into(self) -> Result<$repr, Self::Error> {
                let value: $repr = self.value.0.try_into()?;
                Ok(-(self.negative as $repr) ^ value)
            }
        }
    };
}

impl_int_conversion_cbor_int!(uint u8);
impl_int_conversion_cbor_int!(uint u16);
impl_int_conversion_cbor_int!(uint u32);
impl_int_conversion_cbor_int!(infaillible uint u64);
impl_int_conversion_cbor_int!(uint usize);
impl_int_conversion_cbor_int!(int i8);
impl_int_conversion_cbor_int!(int i16);
impl_int_conversion_cbor_int!(int i32);
impl_int_conversion_cbor_int!(int i64);

impl TryFrom<i128> for CborInteger {
    type Error = std::num::TryFromIntError;

    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn try_from(value: i128) -> Result<Self, Self::Error> {
        let negative = value.is_negative();
        let abs: u64 = value.abs().try_into()?;
        Ok(Self {
            value: abs.into(),
            negative,
        })
    }
}

impl CborSerialize for u8 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if self <= &IB_LIMIT {
            writer.write(&[*self])
        } else {
            writer.write(&[ib::consts::IB_UINT_8, *self])
        }
    }
}

impl CborSerialize for u16 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(u8_v) = u8::try_from(*self) {
            return u8_v.cbor_serialize_to(writer);
        }

        let mut buf = [ib::consts::IB_UINT_16, 0, 0];
        buf[1..].copy_from_slice(&self.to_be_bytes());
        writer.write(&buf)
    }
}

impl CborSerialize for u32 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(u16_v) = u16::try_from(*self) {
            return u16_v.cbor_serialize_to(writer);
        }

        let mut buf = [ib::consts::IB_UINT_32, 0, 0, 0, 0];
        buf[1..].copy_from_slice(&self.to_be_bytes());
        writer.write(&buf)
    }
}

impl CborSerialize for u64 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(u32_v) = u32::try_from(*self) {
            return u32_v.cbor_serialize_to(writer);
        }

        let mut buf = [ib::consts::IB_UINT_64, 0, 0, 0, 0, 0, 0, 0, 0];
        buf[1..].copy_from_slice(&self.to_be_bytes());
        writer.write(&buf)
    }
}

impl CborSerialize for i8 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        let mut ui = (self >> (i8::BITS - 1)) as u8;
        let mt = ui & ib::consts::IB_SMALL_NEGATIVE_UINT;
        ui ^= *self as u8;
        if ui <= IB_LIMIT {
            writer.write(&[mt | ui])
        } else {
            writer.write(&[mt | ib::consts::IB_UINT_8, ui])
        }
    }
}

impl CborSerialize for i16 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(i8_v) = i8::try_from(*self) {
            return i8_v.cbor_serialize_to(writer);
        }

        let mut ui = (self >> (i16::BITS - 1)) as u16;
        let mt = (ui & ib::consts::IB_SMALL_NEGATIVE_UINT as u16) as u8;
        ui ^= *self as u16;
        let mut buf = [mt | ib::consts::IB_UINT_16, 0, 0];
        buf[1..].copy_from_slice(&ui.to_be_bytes());
        writer.write(&buf)
    }
}

impl CborSerialize for i32 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(i16_v) = i16::try_from(*self) {
            return i16_v.cbor_serialize_to(writer);
        }

        let mut ui = (self >> (i32::BITS - 1)) as u32;
        let mt = (ui & ib::consts::IB_SMALL_NEGATIVE_UINT as u32) as u8;
        ui ^= *self as u32;
        let mut buf = [mt | ib::consts::IB_UINT_32, 0, 0, 0, 0];
        buf[1..].copy_from_slice(&ui.to_be_bytes());
        writer.write(&buf)
    }
}

impl CborSerialize for i64 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_serialize_to<W: Write>(&self, writer: &mut W) -> Result<usize, SeaboredSerError> {
        if let Ok(i32_v) = i32::try_from(*self) {
            return i32_v.cbor_serialize_to(writer);
        }

        let mut ui = (self >> (i64::BITS - 1)) as u64;
        let mt = (ui & ib::consts::IB_SMALL_NEGATIVE_UINT as u64) as u8;
        ui ^= *self as u64;
        let mut buf = [mt + ib::consts::IB_UINT_64, 0, 0, 0, 0, 0, 0, 0, 0];
        buf[1..].copy_from_slice(&ui.to_be_bytes());
        writer.write(&buf)
    }
}

impl<'a> CborDeserialize<'a> for u8 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if mt != MajorType::Uint {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint],
            });
        }

        Ok(match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        })
    }
}

impl<'a> CborDeserialize<'a> for u16 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if mt != MajorType::Uint {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint],
            });
        }

        Ok(match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u16,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u16,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        })
    }
}

impl<'a> CborDeserialize<'a> for u32 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if mt != MajorType::Uint {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint],
            });
        }

        Ok(match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u32,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u32,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()? as u32,
            crate::ib::AdditionalInfoAction::Uint32 => reader.read_be_u32()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        })
    }
}

impl<'a> CborDeserialize<'a> for u64 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if mt != MajorType::Uint {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint],
            });
        }

        Ok(match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u64,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u64,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()? as u64,
            crate::ib::AdditionalInfoAction::Uint32 => reader.read_be_u32()? as u64,
            crate::ib::AdditionalInfoAction::Uint64 => reader.read_be_u64()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        })
    }
}

impl<'a> CborDeserialize<'a> for i8 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if !matches!(mt, MajorType::NegativeUint | MajorType::Uint) {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint, MajorType::NegativeUint],
            });
        }
        let value_u8 = match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => *ib,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        };

        Ok(-(matches!(mt, MajorType::NegativeUint) as i8) ^ Self::try_from(value_u8)?)
    }
}

impl<'a> CborDeserialize<'a> for i16 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if !matches!(mt, MajorType::NegativeUint | MajorType::Uint) {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint, MajorType::NegativeUint],
            });
        }
        let value_u16 = match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u16,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u16,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        };

        Ok(-(matches!(mt, MajorType::NegativeUint) as i16) ^ Self::try_from(value_u16)?)
    }
}

impl<'a> CborDeserialize<'a> for i32 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if !matches!(mt, MajorType::NegativeUint | MajorType::Uint) {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint, MajorType::NegativeUint],
            });
        }
        let value_u32 = match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u32,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u32,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()? as u32,
            crate::ib::AdditionalInfoAction::Uint32 => reader.read_be_u32()?,

            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        };

        Ok(-(matches!(mt, MajorType::NegativeUint) as i32) ^ Self::try_from(value_u32)?)
    }
}

impl<'a> CborDeserialize<'a> for i64 {
    #[cfg_attr(feature = "inline-nontrivial", inline)]
    fn cbor_deserialize_from<R: Read<'a>>(reader: &mut R) -> Result<Self, SeaboredDeError<'a>>
    where
        Self: Sized + 'a,
    {
        let ib = InitialByte::cbor_deserialize_from(reader)?;
        let (mt, ai) = ib.mt_ai();
        if !matches!(mt, MajorType::NegativeUint | MajorType::Uint) {
            return Err(SeaboredDeError::IncorrectMajorType {
                actual: mt,
                expected: &[MajorType::Uint, MajorType::NegativeUint],
            });
        }
        let value_u64 = match ai.action()? {
            crate::ib::AdditionalInfoAction::DoNothing => ib.0 as u64,
            crate::ib::AdditionalInfoAction::Uint8 => reader.read_byte()? as u64,
            crate::ib::AdditionalInfoAction::Uint16 => reader.read_be_u16()? as u64,
            crate::ib::AdditionalInfoAction::Uint32 => reader.read_be_u32()? as u64,
            crate::ib::AdditionalInfoAction::Uint64 => reader.read_be_u64()?,
            _ => return Err(SeaboredDeError::IllegalAdditionalInfo(ai.0)),
        };

        Ok(-(matches!(mt, MajorType::NegativeUint) as i64) ^ value_u64 as i64)
    }
}
