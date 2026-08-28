//! The menu bar macOS puts at the top of the screen.
//!
//! Tauri's default menu carries only what the system supplies — About, Copy,
//! Minimise — so once the in-window File/View/Help go away on macOS, Relay's
//! own commands would live nowhere. This builds the real thing: the same
//! commands, in the place a Mac user already looks for them.
//!
//! Every item is a message, not a behaviour. The webview already knows how to
//! open a palette or add a project, and duplicating that here would leave two
//! versions of the same decision to drift apart, so picking an item emits its
//! id and the frontend does what it always did.

use tauri::menu::{AboutMetadata, Menu, MenuItem, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// What the webview hears when an item is picked; the payload is the item id.
pub const PICKED: &str = "menu://picked";

/// The three Help lines that describe the world rather than act on it. They
/// are held so their text can follow the status it reports — a menu that still
/// says "not found" after a sign-in is worse than no menu at all.
pub struct Reported<R: Runtime> {
    pub claude: MenuItem<R>,
    pub cli: MenuItem<R>,
    pub budget: MenuItem<R>,
}

/// The frontend owns the wording — it already formats money and versions for
/// the window — so it hands over finished lines rather than raw status.
#[tauri::command]
pub fn sync_menu<R: Runtime>(
    app: AppHandle<R>,
    claude: String,
    cli: String,
    budget: String,
) -> Result<(), String> {
    let Some(items) = app.try_state::<Reported<R>>() else {
        // No native menu on this platform: the window is still drawing its own.
        return Ok(());
    };
    items.claude.set_text(claude).map_err(|e| e.to_string())?;
    items.cli.set_text(cli).map_err(|e| e.to_string())?;
    items.budget.set_text(budget).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
/// Relay's menu, in macOS order. The first submenu is the application one, so
/// it takes the app's name and the items every Mac app is expected to have.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: &str, label: &str, accel: Option<&str>| -> tauri::Result<MenuItem<R>> {
        let mut b = MenuItemBuilder::with_id(id, label);
        if let Some(a) = accel {
            b = b.accelerator(a);
        }
        b.build(app)
    };

    let about = AboutMetadata {
        name: Some("Relay".into()),
        version: Some(app.package_info().version.to_string()),
        ..Default::default()
    };

    let application = SubmenuBuilder::new(app, "Relay")
        .about(Some(about))
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new-chat", "New chat", Some("CmdOrCtrl+N"))?)
        .separator()
        .item(&item("add-project", "Add a project…", None)?)
        .item(&item("projects", "Projects", None)?)
        .separator()
        .close_window()
        .build()?;

    // The text fields in the app are ordinary inputs, so the system's own
    // editing items are the right ones — they already do the work.
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("palette", "Command palette", Some("CmdOrCtrl+K"))?)
        .separator()
        .item(&item("toggle-sidebar", "Toggle the sidebar", None)?)
        .item(&item("toggle-rail", "Toggle Right now", None)?)
        .item(&item("toggle-theme", "Light or dark theme", None)?)
        .separator()
        .item(&item("trees", "Worktrees", None)?)
        .item(&item("activity", "Activity", None)?)
        .separator()
        .fullscreen()
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .separator()
        .close_window()
        .build()?;

    // Placeholder wording only: the frontend replaces all three as soon as it
    // has bootstrapped, and again whenever the status changes.
    let claude = item("claude-terminal", "Sign in to Claude…", None)?;
    let cli = item("cli-version", "Claude CLI", None)?;
    let budget = item("daily-budget", "Daily budget", None)?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&claude)
        .separator()
        .item(&cli)
        .item(&budget)
        .build()?;

    // The two informational lines are not choices; leaving them clickable
    // would promise something picking them cannot deliver.
    let _ = cli.set_enabled(false);
    let _ = budget.set_enabled(false);

    app.manage(Reported { claude, cli, budget });

    Menu::with_items(app, &[&application, &file, &edit, &view, &window, &help])
}

#[cfg(target_os = "macos")]
/// One place where a pick becomes an event, so the ids stay the contract.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let _ = app.emit(PICKED, id.to_string());
}
