pub mod commands;
pub mod crypto;
pub mod db;
pub mod domain;

use db::Database;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

/// Shared application state accessible from all Tauri commands.
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize logging
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Debug)
                        .build(),
                )?;
            }

            // Initialize database
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");
            std::fs::create_dir_all(&app_dir)
                .expect("failed to create app data directory");

            let db_path = app_dir.join("alexandria.db");
            log::info!("Database path: {}", db_path.display());

            let database =
                Database::open(&db_path).expect("failed to open database");
            database
                .run_migrations()
                .expect("failed to run database migrations");

            log::info!("Database initialized successfully");

            app.manage(AppState {
                db: Arc::new(Mutex::new(database)),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health::check_health,
            commands::identity::generate_wallet,
            commands::identity::get_wallet_info,
            commands::identity::get_profile,
            commands::identity::update_profile,
            commands::courses::list_courses,
            commands::courses::get_course,
            commands::courses::create_course,
            commands::courses::update_course,
            commands::courses::delete_course,
            commands::enrollment::list_enrollments,
            commands::enrollment::enroll,
            commands::enrollment::update_progress,
            commands::enrollment::get_progress,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
