use live_long_and_prospero::ir;

fn main() -> ir::io::Result<()> {
    let size = if let Some(arg) = std::env::args().nth(1) {
        arg.parse().expect("number of pixels wide/tall to render")
    } else {
        512
    };
    let insts = ir::io::read(std::io::stdin().lock(), ir::Insts::default())?;
    ir::interp::interp(std::io::stdout().lock(), &insts, size)?;
    Ok(())
}
