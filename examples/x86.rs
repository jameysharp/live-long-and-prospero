use clap::Parser;
use live_long_and_prospero::codegen;
use live_long_and_prospero::ir;

#[derive(Parser)]
struct Cli {
    /// Split the input program into separate functions according to which
    /// variables they depend on, so that intermediate values only need to be
    /// computed once and can be shared across an entire row or column of the
    /// image. This may increase the number of values which need to be stored in
    /// memory but overall reduces the number of instructions executed.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new())]
    memoize: bool,

    #[command(flatten)]
    config: codegen::x86::X86Config,
}

fn main() -> ir::io::Result<()> {
    let cli = Cli::parse();
    let input = std::io::stdin().lock();
    let memoized = if cli.memoize {
        ir::io::read(input, ir::memoize::MemoBuilder::default())?
    } else {
        ir::io::read(input, ir::memoize::UnmemoBuilder::default())?
    };
    codegen::x86::write(std::io::stdout().lock(), cli.config, &memoized)?;
    Ok(())
}
