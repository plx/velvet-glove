use velvet_glove::scaffold::cli::Cli;

fn main() -> std::process::ExitCode {
    velvet_glove::run(Cli::parse_validated())
}
