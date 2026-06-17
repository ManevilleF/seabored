use crate::ib::InitialByte;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
/// CBOR Major Type
pub enum MajorType {
    Uint = 0,
    NegativeUint = 1,
    Bytes = 2,
    String = 3,
    Array = 4,
    Map = 5,
    Tagged = 6,
    SimpleValueOrFloat = 7,
}

impl std::fmt::Display for MajorType {
    #[inline(always)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MajorType::Uint => write!(f, "MajorType::Uint"),
            MajorType::NegativeUint => write!(f, "MajorType::NegativeUint"),
            MajorType::Bytes => write!(f, "MajorType::Bytes"),
            MajorType::String => write!(f, "MajorType::String"),
            MajorType::Array => write!(f, "MajorType::Array"),
            MajorType::Map => write!(f, "MajorType::Map"),
            MajorType::Tagged => write!(f, "MajorType::Tagged"),
            MajorType::SimpleValueOrFloat => write!(f, "MajorType::SimpleValueOrFloat"),
        }
    }
}

impl From<InitialByte> for MajorType {
    #[inline(always)]
    fn from(ib: InitialByte) -> Self {
        // SAFETY: u8::MAX >> 5 results in 7.
        // And MajorType goes from 0 to 7
        // so it's impossible for this to be UB
        unsafe { std::mem::transmute(ib.0 >> 5u8) }
    }
}
