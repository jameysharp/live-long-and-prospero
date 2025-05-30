use live_long_and_prospero::ir;

fn main() -> ir::io::Result<()> {
    let memoized = ir::io::read(std::io::stdin().lock(), ir::memoize::MemoBuilder::new())?;
    ir::io::write_memoized(std::io::stdout().lock(), &memoized)?;
    Ok(())
}
