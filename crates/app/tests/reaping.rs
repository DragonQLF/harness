//! A limpeza a sério, com processos verdadeiros.
//!
//! Os testes de unidade do `strays` decidem *quem* levar, contra saídas do `ps`
//! copiadas da máquina onde o #108 aconteceu. Isto exercita a outra metade — a
//! que mata — porque é a que estraga se estiver errada, e um filtro certo com
//! um `kill` largo é pior do que nenhum dos dois.
//!
//! Os processos são de mentira mas os nomes são os verdadeiros: o que o
//! `strays` reconhece é o caminho do sidecar e o do binário do SDK, e é por
//! esses que se monta a árvore aqui.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Vivo mesmo — não zombie.
///
/// O `kill(pid, 0)` diz que sim a um zombie, e um filho que ninguém colheu fica
/// zombie: escrito com ele, este teste dizia que a limpeza não tinha funcionado
/// quando tinha. O estado do `ps` é o que separa os dois.
fn alive(pid: u32) -> bool {
    let out = Command::new("ps").args(["-o", "state=", "-p", &pid.to_string()]).output();
    match out {
        Ok(out) => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => false,
    }
}

fn wait_gone(pids: &[u32], limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if pids.iter().all(|p| !alive(*p)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Uma árvore com os nomes que o `strays` procura: um sidecar que levanta um
/// CLI com `--resume` de uma sessão dada.
struct FakeRun {
    dir: std::path::PathBuf,
    sidecar: u32,
    cli: u32,
}

fn spawn_fake(session: &str, tag: &str) -> FakeRun {
    let dir = std::env::temp_dir().join(format!("relay-reap-{tag}-{}", std::process::id()));
    let sidecar_dir = dir.join("sidecar");
    let cli_dir = dir.join("node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64");
    std::fs::create_dir_all(&sidecar_dir).unwrap();
    std::fs::create_dir_all(&cli_dir).unwrap();

    let cli = cli_dir.join("claude");
    std::fs::write(&cli, "#!/bin/sh\nsleep 45\n").unwrap();
    let sidecar = sidecar_dir.join("index.mjs");
    std::fs::write(
        &sidecar,
        format!("#!/bin/sh\n\"{}\" --output-format stream-json --resume=$1 &\necho $!\nsleep 45\n", cli.display()),
    )
    .unwrap();
    for f in [&cli, &sidecar] {
        let mut perm = std::fs::metadata(f).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(f, perm).unwrap();
    }

    let child = Command::new(&sidecar)
        .arg(session)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake sidecar");
    let sidecar_pid = child.id();

    // O pid do "CLI" sai na primeira linha do sidecar de mentira.
    use std::io::BufRead;
    let mut out = std::io::BufReader::new(child.stdout.expect("stdout"));
    let mut line = String::new();
    out.read_line(&mut line).expect("read cli pid");
    let cli_pid: u32 = line.trim().parse().expect("cli pid");

    // Dar ao `ps` um instante para o ver.
    std::thread::sleep(Duration::from_millis(250));
    FakeRun { dir, sidecar: sidecar_pid, cli: cli_pid }
}

impl Drop for FakeRun {
    fn drop(&mut self) {
        for pid in [self.cli, self.sidecar] {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_stray_holding_a_session_is_really_killed_and_so_is_its_sidecar() {
    let session = format!("sess-{}-alvo", std::process::id());
    let run = spawn_fake(&session, "alvo");
    assert!(alive(run.cli) && alive(run.sidecar), "a árvore devia estar de pé");

    let swept = harness_app::strays::reap_session(&session);
    assert_eq!(swept, 1, "um só, e é aquele");

    assert!(
        wait_gone(&[run.cli, run.sidecar], Duration::from_secs(3)),
        "o CLI e o sidecar tinham de cair os dois: matar só um deles é a avaria de origem",
    );
}

/// A metade que interessa mais. Uma sessão que não é a nossa não se toca — e
/// esta é a asserção que separa uma limpeza de um estrago.
#[test]
fn a_run_on_another_session_is_left_standing() {
    let mine = format!("sess-{}-minha", std::process::id());
    let theirs = format!("sess-{}-alheia", std::process::id());
    let keep = spawn_fake(&theirs, "alheia");
    let go = spawn_fake(&mine, "minha");

    let swept = harness_app::strays::reap_session(&mine);
    assert_eq!(swept, 1);
    assert!(wait_gone(&[go.cli, go.sidecar], Duration::from_secs(3)));

    // E o outro continua exactamente onde estava.
    assert!(alive(keep.cli), "o CLI da outra sessão nunca");
    assert!(alive(keep.sidecar), "nem o sidecar dela");
}

/// E uma sessão que ninguém segura não é motivo para matar nada.
#[test]
fn nothing_held_means_nothing_killed() {
    let session = format!("sess-{}-vazia", std::process::id());
    let keep = spawn_fake(&format!("{session}-outra"), "vazia");
    assert_eq!(harness_app::strays::reap_session(&session), 0);
    assert!(alive(keep.cli) && alive(keep.sidecar));
}
