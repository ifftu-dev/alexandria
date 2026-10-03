use alexandria_decisions::evaluation::{evaluate, Example, Manifest};
use alexandria_learning_contracts::TaxonomySnapshot;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: evaluate <manifest.json> <taxonomy.json> <examples.jsonl>".into());
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let taxonomy: TaxonomySnapshot = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let examples: Vec<Example> = std::fs::read_to_string(&args[3])?
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&evaluate(manifest, &taxonomy, &examples)?)?
    );
    Ok(())
}
