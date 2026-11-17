fn main() {
    std::process::exit(xtask::cli::run(std::env::args().skip(1)));
}
