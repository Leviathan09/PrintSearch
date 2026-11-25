//! Cycle de vie de l'application : instance unique, serveur, icône de la barre d'état, arrêt propre.
#[cfg(has_tray)]
use crate::log::log_error;
use crate::log::{log_info, log_warn};
use anyhow::{bail, Result};
use std::{net::TcpListener, sync::OnceLock, time::Duration};
use tokio::sync::watch;

pub const ADDR: &str = "127.0.0.1:8787";
pub const URL: &str = "http://localhost:8787";

static QUIT: OnceLock<watch::Sender<bool>> = OnceLock::new();

fn quit_tx() -> &'static watch::Sender<bool> {
    QUIT.get_or_init(|| watch::channel(false).0)
}

/// Demande l'arrêt propre de l'application (menu de l'icône, commande `quit`, Ctrl+C…).
pub fn request_quit() {
    if !quit_tx().send_replace(true) {
        log_info!("quit requested"); // une seule fois, même si plusieurs sources le demandent
    }
}

/// Se termine quand l'arrêt a été demandé (même si la demande a eu lieu avant l'appel).
pub async fn wait_quit() {
    let mut rx = quit_tx().subscribe();
    let _ = rx.wait_for(|v| *v).await;
}

/// Lance l'application. `tray` : afficher l'icône de la barre d'état (si le système la gère).
/// `foreground` : rester attaché au terminal au lieu de se détacher (Linux).
pub fn run(open: bool, tray: bool, foreground: bool) -> Result<()> {
    // Lancé depuis un terminal, on se détache et on rend la main : pas de fenêtre de terminal à garder ouverte.
    #[cfg(all(unix, has_tray))]
    if tray && !foreground && detach_from_terminal(open)? {
        return Ok(());
    }
    let _ = foreground;
    let listener = match TcpListener::bind(ADDR) {
        Ok(l) => l,
        Err(e) => return already_running(open, e),
    };
    log_info!("PrintSearch {} starting on {} {} ({URL})", crate::update::current(), std::env::consts::OS, std::env::consts::ARCH);

    #[cfg(has_tray)]
    if tray {
        return run_with_tray(listener, open);
    }
    let _ = tray; // sans barre d'état disponible : exécution dans le terminal
    run_foreground(listener, open)
}

/// Unix : si on a été lancé depuis un terminal, relance PrintSearch dans une nouvelle session, sans terminal,
/// attend qu'il réponde, puis rend la main. Renvoie `true` quand le processus courant n'a plus rien à faire.
#[cfg(all(unix, has_tray))]
fn detach_from_terminal(open: bool) -> Result<bool> {
    use std::io::IsTerminal;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let from_terminal = std::io::stdin().is_terminal() || std::io::stderr().is_terminal();
    if std::env::var_os("PRINTSEARCH_DETACHED").is_some() || !from_terminal {
        return Ok(false); // lancé depuis un menu, un service, ou déjà détaché
    }
    crate::log::set_echo(false); // ici le programme parle lui-même : pas de lignes de journal mêlées à ses messages
    if probe() {
        log_info!("PrintSearch is already running: reusing the existing instance");
        println!("PrintSearch is already running ({URL}).");
        if open {
            let _ = ::open::that(URL);
        }
        return Ok(true);
    }
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(std::env::args_os().skip(1))
        .env("PRINTSEARCH_DETACHED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: seul `setsid` est appelé entre fork et exec ; il est sûr dans ce contexte.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid(); // nouvelle session : plus de terminal de contrôle, fermer le terminal ne tue pas l'application
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    log_info!("detached into the background (pid {})", child.id());
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(200));
        if probe() {
            println!("PrintSearch is running in the background ({URL}). Look for its icon in the system tray.");
            println!("To stop it: `printsearch quit`, or Quit in the tray menu.");
            return Ok(true);
        }
    }
    println!("PrintSearch is starting in the background. If nothing appears, run `printsearch logs` to see why.");
    Ok(true)
}

/// Le port est pris : si c'est déjà PrintSearch, on se contente d'ouvrir son interface.
fn already_running(open: bool, e: std::io::Error) -> Result<()> {
    if probe() {
        log_info!("PrintSearch is already running: reusing the existing instance");
        println!("PrintSearch is already running ({URL}).");
        if open {
            let _ = ::open::that(URL);
        }
        return Ok(());
    }
    bail!("port 8787 is not available ({e}); is another program using it?")
}

/// Vrai si l'instance qui écoute sur le port est bien PrintSearch (requête HTTP minimale, sans runtime).
fn probe() -> bool {
    use std::io::{Read, Write};
    let Ok(addr) = ADDR.parse() else { return false };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    if s.write_all(b"GET /api/status HTTP/1.1\r\nHost: localhost:8787\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.take(8192).read_to_string(&mut buf);
    buf.contains("\"makerworld\"") && buf.contains("\"version\"")
}

fn run_foreground(listener: TcpListener, open: bool) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(serve(listener, open))
}

/// Serveur + gestion des signaux ; ne revient qu'après l'arrêt (avec un délai maximal de 6 s).
async fn serve(listener: TcpListener, open: bool) -> Result<()> {
    tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        log_info!("Ctrl+C received");
        request_quit();
    });
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        // SIGTERM (arrêt demandé) et SIGHUP (terminal fermé) : arrêt propre plutôt que brutal
        for (kind, name) in [(SignalKind::terminate(), "SIGTERM"), (SignalKind::hangup(), "SIGHUP")] {
            if let Ok(mut s) = signal(kind) {
                tokio::spawn(async move {
                    s.recv().await;
                    log_info!("{name} received");
                    request_quit();
                });
            }
        }
    }

    let result = tokio::select! {
        r = crate::server::run(listener, open) => r,
        _ = async { wait_quit().await; tokio::time::sleep(Duration::from_secs(6)).await; } => {
            log_warn!("shutdown took too long: exiting anyway");
            Ok(())
        }
    };
    crate::browser::shutdown(); // ferme le navigateur d'arrière-plan : aucun processus orphelin
    log_info!("PrintSearch stopped");
    result
}

/// Serveur dans un thread, icône de la barre d'état dans le thread principal (qui doit faire tourner sa boucle de messages).
#[cfg(has_tray)]
fn run_with_tray(listener: TcpListener, open: bool) -> Result<()> {
    let lang = crate::config::Config::load().lang.unwrap_or_else(|| "en".into());
    // Même si le thread du serveur plante, l'icône doit disparaître (sinon elle resterait affichée sans rien derrière).
    struct StopTrayOnExit;
    impl Drop for StopTrayOnExit {
        fn drop(&mut self) {
            crate::tray::stop();
        }
    }
    let server = std::thread::Builder::new().name("server".into()).spawn(move || -> Result<()> {
        let _stop_tray = StopTrayOnExit;
        let res = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(anyhow::Error::from).and_then(|rt| rt.block_on(serve(listener, open)));
        if let Err(e) = &res {
            log_error!("server stopped with an error: {e:#}");
        }
        res
    })?;

    match crate::tray::run(&lang) {
        Ok(()) => request_quit(), // icône fermée -> arrêt propre du serveur
        Err(e) => log_warn!("tray icon unavailable ({e:#}); PrintSearch keeps running without it (close it from the web interface or with `printsearch quit`)"),
    }
    match server.join() {
        Ok(r) => r,
        Err(_) => bail!("the server thread crashed (see the log)"),
    }
}
