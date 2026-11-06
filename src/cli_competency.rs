//! Store-free competency contract linting.
pub(crate) fn run(args: &[String]) {
    // Admission permits the global --db option anywhere after the command.
    // It is ignored by this store-free operation, including its value.
    let mut tokens = args.iter().skip(1);
    let mut positionals = Vec::new();
    while let Some(token) = tokens.next() {
        if token == "--db" {
            tokens.next();
        } else {
            positionals.push(token);
        }
    }
    let pendant = std::path::Path::new(positionals[2]);
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
