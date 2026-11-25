//! Icône de la barre d'état (Windows) / zone de notification (Linux, protocole StatusNotifierItem).
//! Clic gauche : ouvre l'interface. Clic droit : menu (Ouvrir, Quitter).
//! Sous Linux, l'icône n'apparaît que si votre bureau affiche les icônes de notification
//! (KDE, XFCE, Cinnamon… ; GNOME demande l'extension « AppIndicator »).
use anyhow::{anyhow, Result};
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
};

const ICON_RGBA: &[u8] = include_bytes!("../packaging/tray-64.rgba"); // 64x64, RGBA
static STOP: AtomicBool = AtomicBool::new(false);
static LAST_OPEN_MS: AtomicU64 = AtomicU64::new(0);

/// Demande à `run` de se terminer (appelé quand le serveur s'arrête).
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

fn labels(lang: &str) -> (&'static str, &'static str) {
    match lang {
        "fr" => ("Ouvrir PrintSearch", "Quitter"),
        "de" => ("PrintSearch öffnen", "Beenden"),
        _ => ("Open PrintSearch", "Quit"),
    }
}

#[cfg(test)]
mod labels_tests {
    use super::labels;

    #[test]
    fn labels_de() {
        assert_eq!(labels("de"), ("PrintSearch öffnen", "Beenden"));
        assert_eq!(labels("fr").1, "Quitter");
        assert_eq!(labels("en").1, "Quit");
        assert_eq!(labels("xx").1, "Quit");
    }
}

/// Ouvre l'interface dans le navigateur (au plus une fois par seconde : un double-clic envoie deux clics).
fn open_ui() {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    if now.saturating_sub(LAST_OPEN_MS.swap(now, Ordering::SeqCst)) < 1000 {
        return;
    }
    let _ = open::that(crate::app::url());
}

/// Crée l'icône puis bloque jusqu'à `stop()`. Renvoie une erreur si l'icône ne peut pas être créée.
pub fn run(lang: &str) -> Result<()> {
    let (l_open, l_quit) = labels(lang);
    let menu = Menu::new();
    let open_item = MenuItem::new(l_open, true, None);
    let quit_item = MenuItem::new(l_quit, true, None);
    menu.append_items(&[&open_item, &PredefinedMenuItem::separator(), &quit_item]).map_err(|e| anyhow!("{e}"))?;
    let (id_open, id_quit) = (open_item.id().clone(), quit_item.id().clone());

    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == id_open {
            open_ui();
        } else if e.id == id_quit {
            crate::app::request_quit();
        }
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            open_ui();
        }
    }));

    let icon = Icon::from_rgba(ICON_RGBA.to_vec(), 64, 64).map_err(|e| anyhow!("{e}"))?;
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("PrintSearch")
        .with_title("PrintSearch")
        .with_icon(icon)
        .build()
        .map_err(|e| anyhow!("{e}"))?;
    crate::log::log_info!("tray icon created");
    wait_for_stop();
    Ok(())
}

/// Windows : l'icône vit dans la boucle de messages de ce thread, qu'il faut donc faire tourner.
#[cfg(windows)]
fn wait_for_stop() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT, WM_QUIT};
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        loop {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if STOP.load(Ordering::SeqCst) {
                return;
            }
            MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 100, QS_ALLINPUT);
        }
    }
}

/// Linux : le service D-Bus de l'icône tourne dans son propre thread ; il suffit d'attendre.
#[cfg(not(windows))]
fn wait_for_stop() {
    while !STOP.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}
