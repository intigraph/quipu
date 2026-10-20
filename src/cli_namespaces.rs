//! Local owner registry application and stable-reference commands.

pub fn run(args: &[String], db_path: &str) {
    let result = (|| -> quipu::Result<serde_json::Value> {
        let action = args.get(2).map_or("list", String::as_str);
        let mut store = crate::cli_open::open_store(db_path);
        match (args[1].as_str(), action) {
            ("namespaces", "list") => {
                quipu::entity_reference::tool_namespaces(&store, &serde_json::json!({}))
            }
            ("namespaces", "apply") => quipu::entity_namespaces::apply(&mut store),
            ("entity-ref", "mint" | "resolve") => {
                let path = args.get(3).ok_or_else(|| {
                    quipu::Error::InvalidValue(
                        "entity-ref mint|resolve needs a request JSON file".into(),
                    )
                })?;
                let text = std::fs::read_to_string(path).map_err(|e| {
                    quipu::Error::InvalidValue(format!("cannot read request file: {e}"))
                })?;
                let mut input: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| quipu::Error::InvalidValue(e.to_string()))?;
                let object = input.as_object_mut().ok_or_else(|| {
                    quipu::Error::InvalidValue("request must be an object".into())
                })?;
                if object.get("action").is_some_and(|value| value != action) {
                    return Err(quipu::Error::InvalidValue(
                        "conflicting request action".into(),
                    ));
                }
                object.insert("action".into(), action.into());
                quipu::entity_reference::tool_entity_ref(&mut store, &input)
            }
            _ => Err(quipu::Error::InvalidValue(
                "use namespaces list|apply or entity-ref mint|resolve <file.json>".into(),
            )),
        }
    })();
    match result {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).expect("JSON")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
