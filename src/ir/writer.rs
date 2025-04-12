use std::io;

use super::Inst;

pub fn write(mut f: impl io::Write, insts: impl IntoIterator<Item = Inst>) -> io::Result<()> {
    for (idx, inst) in insts.into_iter().enumerate() {
        write!(f, "v{} ", idx)?;
        match inst {
            Inst::Const { value } => writeln!(f, "const {value}")?,
            Inst::Var { idx } => writeln!(f, "var-{}", char::from(b"xyzabcde"[usize::from(idx)]))?,
            Inst::Load => writeln!(f, "load")?,
            Inst::UnOp { op, arg } => writeln!(f, "{} v{arg}", op.name())?,
            Inst::BinOp { op, args: [a, b] } => writeln!(f, "{} v{a} v{b}", op.name())?,
        };
    }
    Ok(())
}
