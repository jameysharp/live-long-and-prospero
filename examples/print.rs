use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock(), ir::Insts::default())?;
    ir::io::write(std::io::stdout().lock(), insts.pool.into_iter())?;
    Ok(())
}
