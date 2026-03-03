use std::path::{Path, PathBuf};
use std::sync::Arc;

use app_lib::db::{seed, seed_content, Database};
use app_lib::ipfs::node::ContentNode;

fn usage() {
    eprintln!(
        "Usage:\n  cargo run --manifest-path src-tauri/Cargo.toml --features dev-seed --bin dev_seed -- <up|down|reset> [--db PATH] [--iroh PATH]\n\nDefaults:\n  --db   ./dev-data/alexandria.db\n  --iroh ./dev-data/iroh"
    );
}

fn parse_paths(args: &[String]) -> (PathBuf, PathBuf) {
    let mut db_path = PathBuf::from("./dev-data/alexandria.db");
    let mut iroh_dir = PathBuf::from("./dev-data/iroh");

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--db" if i + 1 < args.len() => {
                db_path = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--iroh" if i + 1 < args.len() => {
                iroh_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    (db_path, iroh_dir)
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
        println!("Removed {}", path.display());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return Err("missing command".into());
    }

    let cmd = args[1].as_str();
    let (db_path, iroh_dir) = parse_paths(&args[2..]);

    match cmd {
        "down" | "reset" => {
            remove_file_if_exists(&db_path)?;
            let wal = PathBuf::from(format!("{}-wal", db_path.display()));
            let shm = PathBuf::from(format!("{}-shm", db_path.display()));
            remove_file_if_exists(&wal)?;
            remove_file_if_exists(&shm)?;

            if iroh_dir.exists() {
                std::fs::remove_dir_all(&iroh_dir)
                    .map_err(|e| format!("failed to remove {}: {e}", iroh_dir.display()))?;
                println!("Removed {}", iroh_dir.display());
            }
            if cmd == "down" {
                return Ok(());
            }
        }
        "up" => {}
        _ => {
            usage();
            return Err(format!("unknown command: {cmd}"));
        }
    }

    ensure_parent(&db_path)?;
    std::fs::create_dir_all(&iroh_dir)
        .map_err(|e| format!("failed to create {}: {e}", iroh_dir.display()))?;

    let database = Database::open(&db_path).map_err(|e| e.to_string())?;
    database.run_migrations().map_err(|e| e.to_string())?;

    let seeded = seed::seed_if_empty(database.conn()).map_err(|e| e.to_string())?;
    println!(
        "Database seed: {}",
        if seeded { "inserted" } else { "already present" }
    );

    let db = Arc::new(std::sync::Mutex::new(database));
    let node = Arc::new(ContentNode::new(&iroh_dir));
    node.start()
        .await
        .map_err(|e| format!("failed to start iroh node: {e}"))?;

    let updated = seed_content::seed_content_if_needed(&db, &node).await?;
    println!("Seeded iroh content for {updated} elements");

    node.shutdown()
        .await
        .map_err(|e| format!("failed to stop iroh node: {e}"))?;

    Ok(())
}
