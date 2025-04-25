use std::num::{NonZero, TryFromIntError};

use crate::ir::VarSet;

pub mod regalloc;
pub mod x86;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Register(NonZero<u8>);

impl TryFrom<usize> for Register {
    type Error = TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u8::try_from(value.wrapping_add(1))
            .and_then(NonZero::try_from)
            .map(Register)
    }
}

impl Register {
    pub fn idx(self) -> usize {
        usize::from(self.0.get()) - 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemorySpace(NonZero<u8>);

impl MemorySpace {
    pub const STACK: Self = Self(NonZero::<u8>::MIN);

    pub fn idx(self) -> usize {
        usize::from(self.0.get()) - 1
    }
}

impl From<VarSet> for MemorySpace {
    fn from(value: VarSet) -> Self {
        Self(NonZero::new(u8::try_from(value.idx() + 2).unwrap()).unwrap())
    }
}
