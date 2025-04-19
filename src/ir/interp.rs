use std::io;

use super::{BinOp, Inst, Insts, UnOp};

pub fn interp(mut f: impl io::Write, insts: &Insts, size: u16) -> io::Result<()> {
    // https://netpbm.sourceforge.net/doc/pbm.html
    writeln!(f, "P4 {size} {size}")?;

    let mut row = vec![0u8; (usize::from(size) + 7) / 8];
    let mut regs = vec![0f32; insts.len()];
    let mut vars = [0f32; 2];
    let scale = 2.0 / f32::from(size - 1);

    for y in (0..size).rev() {
        vars[1] = f32::from(y) * scale - 1.0;
        for x in 0..size {
            vars[0] = f32::from(x) * scale - 1.0;

            for (idx, inst) in insts.iter().enumerate() {
                regs[idx] = match *inst {
                    Inst::Const { value } => value.value(),
                    Inst::Var { var } => vars[var as usize],
                    Inst::UnOp { op, arg } => {
                        let arg = regs[arg.idx()];
                        match op {
                            UnOp::Neg => -arg,
                            UnOp::Square => arg * arg,
                            UnOp::Sqrt => arg.sqrt(),
                        }
                    }
                    Inst::BinOp { op, args: [a, b] } => {
                        let a = regs[a.idx()];
                        let b = regs[b.idx()];
                        match op {
                            BinOp::Add => a + b,
                            BinOp::Sub => a - b,
                            BinOp::Mul => a * b,
                            BinOp::Min => a.min(b),
                            BinOp::Max => a.max(b),
                        }
                    }
                    Inst::Load { .. } => unimplemented!("load instruction in interpreter"),
                };
            }

            if regs.last().unwrap().is_sign_positive() {
                row[usize::from(x >> 3)] |= 0x80 >> (x & 7);
            }
        }

        f.write_all(&row)?;
        row.fill(0);
    }

    Ok(())
}
