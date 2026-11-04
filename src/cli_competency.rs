//! Store-free competency contract linting.
pub(crate) fn run(args: &[String]) {
    let pendant = std::path::Path::new(&args[3]);
    match quipu::competency::load(pendant) {
        Ok(contract) => println!(
            "competency: {} question(s), {} command(s); schema and syntax pass (not executed)",
            contract.questions.len(),
            contract.commands.len()
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
