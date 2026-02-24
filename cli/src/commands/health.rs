use anyhow::Result;
use sysinfo::System;

use crate::context::ProjectContext;
use crate::output;

pub fn execute(ctx: &ProjectContext) -> Result<()> {
    output::header("Health check");

    // Check if Alexandria process is running
    let s = System::new_all();
    let mut found = false;
    for process in s.processes().values() {
        let name = process.name().to_string_lossy().to_lowercase();
        if name.contains("alexandria") {
            output::success(&format!(
                "Alexandria process running (PID {}, {})",
                process.pid(),
                name
            ));
            found = true;
        }
    }
    if !found {
        output::info("No Alexandria process detected");
    }

    output::blank();
    output::header("App data");

    if ctx.has_app_data() {
        output::success("App data directory exists");
    } else {
        output::warning("App data directory missing (app never launched)");
    }

    if ctx.has_db() {
        output::success("Database exists");
    } else {
        output::warning("Database not created");
    }

    if ctx.has_vault() {
        output::success("Vault exists");
    } else {
        output::warning("Vault not created");
    }

    if ctx.iroh_dir().exists() {
        output::success("Iroh store exists");
    } else {
        output::warning("Iroh store not created");
    }

    output::blank();
    Ok(())
}
