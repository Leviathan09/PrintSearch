//! Cycle de vie de l'application : instance unique, serveur, icône de la barre d'état, arrêt propre.
#[cfg(has_tray)]
use crate::log::log_error;
use crate::log::{log_info, log_warn};
use anyhow::{bail, Result};
use crate::server::{bind_addr, display_host, DEFAULT_BIND, PORT};
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::watch;

/// Address a user types in a browser: always an authority the Host/Origin allow-list accepts
/// (`localhost` for the default or a wildcard bind, otherwise the configured address or name).
pub fn url() -> String {
    format!("http://{}:{PORT}", display_host(&bind_addr()))
}

/// True for an address of this very machine: loopback, or one we are able to bind a socket to.
fn is_local_ip(ip: std::net::IpAddr) -> bool {
    ip.is_loopback() || std::net::UdpSocket::bind((ip, 0)).is_ok()
}

/// Where the CLI and the duplicate-start check look for a running instance. Only addresses of this machine are
/// ever contacted, even when `PRINTSEARCH_BIND` names a host that resolves to several (or remote) addresses.
fn client_targets() -> Vec<SocketAddr> {
    let bind = bind_addr();
    if bind == DEFAULT_BIND || bind == "0.0.0.0" {
        return vec![(Ipv4Addr::LOCALHOST, PORT).into()];
    }
    if bind == "::" {
        return vec![(Ipv6Addr::LOCALHOST, PORT).into()];
    }
    // Name resolution can block for a long time: give it a deadline of its own.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::net::ToSocketAddrs;
        let found = (bind.as_str(), PORT).to_socket_addrs().map(|i| i.collect::<Vec<_>>()).unwrap_or_default();
        let _ = tx.send(found);
    });
    let mut v: Vec<SocketAddr> = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default().into_iter().filter(|a| is_local_ip(a.ip())).collect();
    v.sort();
    v.dedup();
    v
}

/// A raw HTTP answer is accepted as PrintSearch only if it is `200` with a JSON object that has a string
/// `version` and a boolean `makerworld`: two marker words in any text are not enough.
fn is_printsearch_status(raw: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(raw) else { return false };
    if !(text.starts_with("HTTP/1.1 200") || text.starts_with("HTTP/1.0 200")) {
        return false;
    }
    let Some((_, body)) = text.split_once("\r\n\r\n") else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) else { return false };
    v.get("version").map_or(false, |x| x.is_string()) && v.get("makerworld").map_or(false, |x| x.is_boolean())
}

/// The address of the running PrintSearch of this machine, if any (one deadline for the whole check).
pub fn probe() -> Option<SocketAddr> {
    use std::io::{Read, Write};
    let deadline = Instant::now() + Duration::from_secs(3);
    for addr in client_targets() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, left.min(Duration::from_secs(1))) else { continue };
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(200));
        let _ = s.set_read_timeout(Some(left));
        let _ = s.set_write_timeout(Some(left));
        if s.write_all(b"GET /api/status HTTP/1.1\r\nHost: localhost:8787\r\nConnection: close\r\n\r\n").is_err() {
            continue;
        }
        let mut buf = Vec::new();
        let _ = (&s).take(16384).read_to_end(&mut buf);
        if is_printsearch_status(&buf) {
            return Some(addr);
        }
    }
    None
}

pub async fn probe_async() -> Option<SocketAddr> {
    tokio::task::spawn_blocking(probe).await.ok().flatten()
}

/// Asks the running instance of this machine to quit. True only for an HTTP success carrying `{"ok":true}`.
/// Dedicated client: no proxy, no redirect, short timeout, `Host: localhost:8787` (always accepted).
pub async fn local_quit() -> bool {
    let Some(addr) = probe_async().await else { return false };
    let Ok(client) = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(5)).build() else {
        return false;
    };
    let Ok(resp) = client.post(format!("http://{addr}/api/quit")).header("Host", "localhost:8787").send().await else { return false };
    resp.status().is_success() && resp.json::<serde_json::Value>().await.ok().and_then(|v| v.get("ok").and_then(|b| b.as_bool())) == Some(true)
}

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
    let bind = bind_addr();
    let listener = match TcpListener::bind((bind.as_str(), PORT)) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => return already_running(open, e),
        Err(e) => bail!("cannot listen on {bind}:{PORT} (is the address valid and assigned to this machine, and is the port free?): {e}"),
    };
    log_info!("PrintSearch {} starting on {} {} ({})", crate::update::current(), std::env::consts::OS, std::env::consts::ARCH, url());

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
    if probe().is_some() {
        log_info!("PrintSearch is already running: reusing the existing instance");
        println!("PrintSearch is already running ({}).", url());
        if open {
            let _ = ::open::that(url());
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
    let mut child = cmd.spawn()?;
    log_info!("detached into the background (pid {})", child.id());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if let Ok(Some(status)) = child.try_wait() {
            bail!("PrintSearch could not start in the background ({status}); run `printsearch logs` to see why");
        }
        if probe().is_some() {
            println!("PrintSearch is running in the background ({}). Look for its icon in the system tray.", url());
            println!("To stop it: `printsearch quit`, or Quit in the tray menu.");
            return Ok(true);
        }
    }
    println!("PrintSearch is starting in the background. If nothing appears, run `printsearch logs` to see why.");
    Ok(true)
}

/// The port is taken: if it is PrintSearch itself, just open its interface.
fn already_running(open: bool, e: std::io::Error) -> Result<()> {
    if probe().is_some() {
        log_info!("PrintSearch is already running: reusing the existing instance");
        println!("PrintSearch is already running ({}).", url());
        if open {
            let _ = ::open::that(url());
        }
        return Ok(());
    }
    bail!("port {PORT} is not available ({e}); is another program using it?")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn http(body: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[test]
    fn real_status_document_is_accepted() {
        assert!(is_printsearch_status(&http(r#"{"thingiverse":false,"makerworld":false,"version":"0.3.0"}"#)));
    }

    #[test]
    fn marker_words_alone_are_not_enough() {
        assert!(!is_printsearch_status(&http(r#""makerworld" "version" nonsense"#)));
        assert!(!is_printsearch_status(&http(r#"["makerworld","version"]"#)));
        assert!(!is_printsearch_status(&http(r#"{"makerworld":"yes","version":"0.3.0"}"#)));
        assert!(!is_printsearch_status(&http(r#"{"makerworld":false,"version":3}"#)));
        assert!(!is_printsearch_status(b"HTTP/1.1 403 Forbidden\r\n\r\n{\"makerworld\":false,\"version\":\"1\"}"));
        assert!(!is_printsearch_status(b"no http at all"));
    }
}
