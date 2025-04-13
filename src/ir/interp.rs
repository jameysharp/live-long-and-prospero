use std::io;

use super::{BinOp, Inst, Insts, UnOp};

pub fn interp(mut f: impl io::Write, insts: &Insts, size: u16) -> io::Result<()> {
    writeln!(f, "P5 {size} {size} 255")?;

    let mut row = Vec::with_capacity(usize::from(size));
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
                    Inst::Var { var } => vars[usize::from(var.0)],
                    Inst::UnOp { op, arg } => {
                        let arg = regs[usize::from(arg)];
                        match op {
                            UnOp::Neg => -arg,
                            UnOp::Square => arg * arg,
                            UnOp::Sqrt => arg.sqrt(),
                        }
                    }
                    Inst::BinOp { op, args: [a, b] } => {
                        let a = regs[usize::from(a)];
                        let b = regs[usize::from(b)];
                        match op {
                            BinOp::Add => a + b,
                            BinOp::Sub => a - b,
                            BinOp::Mul => a * b,
                            BinOp::Min => a.min(b),
                            BinOp::Max => a.max(b),
                        }
                    }
                    Inst::Load => unimplemented!("load instruction in interpreter"),
                };
            }

            row.push(if *regs.last().unwrap() < 0.0 { 255 } else { 0 });
        }

        f.write_all(&row)?;
        row.clear();
    }

    Ok(())
}
