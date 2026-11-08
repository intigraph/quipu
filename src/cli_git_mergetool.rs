//! Interactive adapter over the existing, store-free pendant resolver.
use std::io::{BufRead, Write};
use std::path::Path;

fn tool(
    args: &[String],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool, String> {
    let by = crate::cli::flag_value(args, "--by")
        .filter(|by| !by.trim().is_empty())
        .ok_or("git-mergetool requires --by <who>")?;
    let path = Path::new(&args[2]);
    if path.file_name().and_then(|p| p.to_str()) != Some("export.nt") {
        return Err("git-mergetool takes a pendant's repository-relative export.nt".into());
    }
    let dir = path
        .parent()
        .and_then(Path::to_str)
        .ok_or("invalid pendant path")?;
    let repo = Path::new(".");
    let [base, ours, theirs] = quipu::git_merge::merge_refs(repo).map_err(|e| e.to_string())?;
    loop {
        let ready = quipu::git_merge::reconcile(repo, dir).map_err(|e| e.to_string())?;
        let decisions = Path::new(dir).join("decisions.json");
        let file = quipu::merge_reconcile::ReconcileFile::load_path(&decisions)
            .map_err(|e| e.to_string())?;
        let view = file.view(
            &decisions.to_string_lossy(),
            quipu::merge_reconcile::Surface::Cli,
        );
        write!(output, "{}", quipu::merge_reconcile::render_text(&view))
            .map_err(|e| e.to_string())?;
        if ready {
            return Ok(true);
        }
        writeln!(
            output,
            "Enter <row-id> <choice>, or quit; no choice is automatic:"
        )
        .map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line.trim() == "quit" {
            return Ok(false);
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let [key, choice] = fields.as_slice() else {
            writeln!(output, "Use exactly <row-id> <choice>.").map_err(|e| e.to_string())?;
            continue;
        };
        let choices = view
            .conflicts
            .iter()
            .find(|row| row.id == *key)
            .map(|row| &row.choices)
            .or_else(|| {
                view.aliases
                    .iter()
                    .find(|row| row.id == *key)
                    .map(|row| &row.choices)
            });
        if !choices.is_some_and(|choices| choices.iter().any(|value| value == choice)) {
            writeln!(output, "Choose a row and choice offered in the layout.")
                .map_err(|e| e.to_string())?;
            continue;
        }
        let author = format!("{by} at {}", crate::cli::chrono_now());
        if quipu::git_merge::merge_refs(repo).map_err(|e| e.to_string())?
            != [base.clone(), ours.clone(), theirs.clone()]
        {
            return Err(
                "merge inputs changed while awaiting a choice; restart the mergetool".into(),
            );
        }
        quipu::git_merge::resolve_by(repo, &base, &ours, &theirs, dir, key, choice, Some(&author))
            .map_err(|e| e.to_string())?;
    }
}

pub fn run(args: &[String]) {
    match tool(
        args,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    ) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("pendant decisions remain; mergetool did not finish");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("pendant mergetool refused: {error}");
            std::process::exit(1);
        }
    }
}
