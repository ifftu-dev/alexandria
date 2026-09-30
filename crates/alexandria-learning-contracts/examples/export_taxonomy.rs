use alexandria_learning_contracts::TaxonomySnapshot;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: export_taxonomy <network-profile.json> <public_taxonomy.json>".into());
    }
    let profile: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let network = profile["network_id"].as_str().ok_or("missing network_id")?;
    let snapshot = TaxonomySnapshot::from_reference(
        network,
        "bundled-v1",
        &std::fs::read_to_string(&args[2])?,
    )?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
