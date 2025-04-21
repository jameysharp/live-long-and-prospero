use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let sink = ir::simplify::Simplify::new(ir::Insts::default());
    let insts = ir::io::read(std::io::stdin().lock(), sink)?;
    ir::io::write(std::io::stdout().lock(), insts.pool.iter().cloned())?;
    Ok(())
}
