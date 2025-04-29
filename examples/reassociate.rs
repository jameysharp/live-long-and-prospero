use live_long_and_prospero::ir;

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock(), ir::Insts::default())?;
    let insts = ir::reassociate::reassociate(&insts.pool, ir::Insts::default());
    ir::io::write(std::io::stdout().lock(), insts.pool.iter().cloned())?;
    Ok(())
}
