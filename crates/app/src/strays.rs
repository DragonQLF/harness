//! Quem está agarrado a uma sessão, e que direito tem a estar.
//!
//! Uma sessão do CLI só pode ter um dono: o processo que a segura escreve nela,
//! e um segundo que a retome não começa turno nenhum — entrega o pedido à fila
//! do primeiro e sai. A mensagem chega; a resposta sai por um stream que já
//! ninguém lê. Foi assim que uma conversa passou dez horas a recusar tudo o que
//! lhe escreviam, sem erro nenhum até à 0.3.16 (#108).
//!
//! O critério não é uma heurística, é estrutural: **um processo agarrado a uma
//! sessão só é legítimo enquanto a Relay tiver uma execução viva para ela.**
//! Sem execução viva não há ninguém a ler o que ele escreve — não está a servir
//! nada nem a ninguém, esteja ou não a gastar CPU. Foi exactamente o caso: às
//! 23:13 a execução acabou em condições e o processo ficou, vivo e inútil, até
//! de manhã.
//!
//! Por isso não há ficheiro de lock. Um lock é uma afirmação sobre o passado e
//! mente de três maneiras — pid reaproveitado, processo que morreu sem limpar,
//! ficheiro escrito por uma Relay que já não existe. Isto pergunta ao sistema
//! *agora*, e o id da sessão vem no `--resume` do próprio processo: ele
//! identifica-se sozinho.

/// Uma linha da tabela de processos, já partida.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub command: String,
}

/// O que só o nosso CLI tem no comando. Sem isto apanhávamos o Claude Code que
/// a operadora tem aberto no terminal dela — outro binário, outra sessão, e
/// nada que nos diga respeito.
const OURS: &str = "claude-agent-sdk";
/// O sidecar que o levanta. É ele o líder do grupo desde a 0.3.16, e é isso que
/// torna seguro levar o grupo em vez do processo.
const SIDECAR: &str = "sidecar/index.mjs";

/// `ps` na forma que se lê aqui. Separado da leitura para o parser poder ser
/// exercitado contra saídas verdadeiras, que é onde isto se parte.
pub fn parse_ps(output: &str) -> Vec<Row> {
    output.lines().filter_map(parse_row).collect()
}

/// Três números e depois o comando inteiro.
///
/// Escrito à mão porque o `ps` alinha as colunas à direita: entre os campos há
/// corridas de espaços, e um `splitn` por espaço singular devolvia campos
/// vazios. O comando não pode ser partido — tem espaços lá dentro, e um deles
/// é o `Application Support` por onde passa todo o caminho que nos interessa.
fn parse_row(line: &str) -> Option<Row> {
    let s = line.trim_start();
    let bytes = s.as_bytes();
    let mut at = 0usize;
    let mut nums = [0u32; 3];
    for slot in nums.iter_mut() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        *slot = s.get(start..at)?.parse().ok()?;
    }
    let command = s.get(at..)?.trim().to_string();
    (!command.is_empty()).then_some(Row {
        pid: nums[0],
        ppid: nums[1],
        pgid: nums[2],
        command,
    })
}

/// Segura esta linha a sessão dada?
///
/// As duas metades fazem falta. O `--resume` sozinho apanharia qualquer CLI da
/// operadora que por acaso tivesse aquele id; o binário sozinho apanharia todos
/// os nossos, incluindo os que estão a servir outras conversas.
fn holds(row: &Row, session_id: &str) -> bool {
    row.command.contains(OURS) && resumes(&row.command, session_id)
}

/// O `--resume` desta linha nomeia exactamente esta sessão?
///
/// Comparado até ao fim do argumento e não por `contains`. Um `contains` fazia
/// `--resume=abc` casar com `--resume=abcdef`, e o que estava do outro lado
/// desse engano era um `SIGKILL` numa sessão que não era a nossa. Encontrado
/// pelo teste que levanta processos a sério, não por leitura.
fn resumes(command: &str, session_id: &str) -> bool {
    for sep in ["--resume=", "--resume "] {
        let mut rest = command;
        while let Some(at) = rest.find(sep) {
            let after = &rest[at + sep.len()..];
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if &after[..end] == session_id {
                return true;
            }
            rest = &after[end..];
        }
    }
    false
}

/// Quem segura esta sessão, agora.
pub fn holders_in<'a>(rows: &'a [Row], session_id: &str) -> Vec<&'a Row> {
    rows.iter().filter(|r| holds(r, session_id)).collect()
}

/// Todos os nossos CLIs, seja qual for a sessão.
pub fn all_ours(rows: &[Row]) -> Vec<&Row> {
    rows.iter().filter(|r| r.command.contains(OURS)).collect()
}

/// Sidecars que já não têm quem os levantou: `ppid` 1, adoptados pelo init.
///
/// É este o critério do arranque, e não "todos os nossos". A tentação era
/// varrer tudo — ao arranque esta Relay não tem execução viva nenhuma, logo
/// nada seria dela. Mas *nada dela* não é o mesmo que *de ninguém*: uma segunda
/// Relay aberta ao mesmo tempo tem os turnos dela a correr, e varrer tudo
/// matava-lhos a meio. Um sidecar com pai vivo é de alguém; um com `ppid` 1
/// não é de ninguém, e essa é a diferença que se pode provar sem adivinhar
/// quem é o dono.
pub fn orphaned_sidecars(rows: &[Row]) -> Vec<&Row> {
    rows.iter()
        .filter(|r| r.ppid == 1 && r.command.contains(SIDECAR))
        .collect()
}

/// O sidecar e os CLIs que ele levantou.
fn brood<'a>(rows: &'a [Row], sidecar: &Row) -> Vec<&'a Row> {
    rows.iter()
        .filter(|r| {
            r.pid == sidecar.pid || (r.ppid == sidecar.pid && r.command.contains(OURS))
        })
        .collect()
}

/// Pode levar-se o grupo inteiro deste processo?
///
/// Só quando o líder do grupo é um sidecar nosso — isto é, quando o grupo foi
/// criado pelo `process_group(0)` da 0.3.16. Nos restos anteriores a essa
/// versão o grupo é o da própria Relay, e levá-lo matava a aplicação. Nesses
/// mata-se processo a processo, que é mais lento mas nunca é largo demais.
pub fn group_is_ours(rows: &[Row], pgid: u32) -> bool {
    rows.iter()
        .any(|r| r.pid == pgid && r.command.contains(SIDECAR))
}

/// O processo e quem o levantou: o CLI e o sidecar que é pai dele. Matar só o
/// CLI deixava o sidecar de pé, e o contrário deixava o CLI órfão — que é a
/// avaria de origem outra vez.
pub fn kin<'a>(rows: &'a [Row], row: &Row) -> Vec<&'a Row> {
    rows.iter()
        .filter(|r| r.pid == row.pid || (r.pid == row.ppid && r.command.contains(SIDECAR)))
        .collect()
}

#[cfg(unix)]
fn read_ps() -> Vec<Row> {
    let out = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,args="])
        .output();
    match out {
        Ok(out) => parse_ps(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Windows não está coberto, e é melhor dizê-lo do que fingir.
///
/// A enumeração lá é WMI e não `ps`, e o pouco que se ganharia em escrevê-la às
/// cegas não paga o risco: um filtro errado aqui não devolve um resultado
/// errado, mata um processo da operadora. Fica a detecção do turno vazio, que é
/// de plataforma nenhuma e continua a dizer-lhe o que se passa.
#[cfg(not(unix))]
fn read_ps() -> Vec<Row> {
    Vec::new()
}

#[cfg(unix)]
fn kill_group(pgid: u32) {
    unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(unix)]
fn kill_one(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(not(unix))]
fn kill_group(_pgid: u32) {}
#[cfg(not(unix))]
fn kill_one(_pid: u32) {}

/// Derruba os restos que seguram esta sessão. Devolve quantos eram.
///
/// Só se chama onde já se sabe que a Relay não tem execução viva para ela —
/// depois de a guarda das duas execuções ter deixado passar esta. Chamado noutro
/// sítio, isto mataria um turno a sério.
pub fn reap_session(session_id: &str) -> usize {
    let rows = read_ps();
    reap(&rows, &holders_in(&rows, session_id))
}

/// Restos deixados por uma Relay que morreu a meio — force quit, crash, o
/// instalador a reiniciá-la. Só os que já não têm pai; ver `orphaned_sidecars`.
pub fn reap_all_on_start() -> usize {
    let rows = read_ps();
    let orphans = orphaned_sidecars(&rows);
    for sidecar in &orphans {
        if group_is_ours(&rows, sidecar.pgid) {
            kill_group(sidecar.pgid);
        } else {
            for r in brood(&rows, sidecar) {
                kill_one(r.pid);
            }
        }
    }
    orphans.len()
}

fn reap(rows: &[Row], strays: &[&Row]) -> usize {
    for stray in strays {
        if group_is_ours(rows, stray.pgid) {
            kill_group(stray.pgid);
        } else {
            for r in kin(rows, stray) {
                kill_one(r.pid);
            }
        }
    }
    strays.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saída verdadeira, copiada da máquina onde o #108 aconteceu: o resto que
    /// segurou a conversa dez horas, o sidecar que o levantou, a Relay, e o
    /// Claude Code que a operadora tinha aberto no terminal dela.
    const REAL: &str = "\
14791 14786  2250 /Users/f/Library/Application Support/com.harness.app/sidecar/node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64/claude --output-format stream-json --resume=f105a514-0126-4ea2-9d49-538fe80e9e28 --permission-mode manual
14786     1  2250 node /Users/f/Library/Application Support/com.harness.app/sidecar/index.mjs
25827     1  2250 /Applications/Relay.app/Contents/MacOS/relay
 4732     1  4700 /Users/f/Library/Application Support/Claude/claude-code/2.1.247/claude.app/Contents/MacOS/claude --output-format stream-json --verbose
";

    #[test]
    fn the_stray_is_found_by_the_session_it_names() {
        let rows = parse_ps(REAL);
        assert_eq!(rows.len(), 4);
        let held = holders_in(&rows, "f105a514-0126-4ea2-9d49-538fe80e9e28");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].pid, 14791);
    }

    /// A que mais interessa. O Claude Code da operadora não é nosso, e um filtro
    /// que o apanhasse deixava de ser uma limpeza para passar a ser um estrago.
    #[test]
    fn the_operators_own_claude_is_never_ours() {
        let rows = parse_ps(REAL);
        assert!(all_ours(&rows).iter().all(|r| r.pid != 4732));
        // Nem sequer quando o id da sessão calha ser o mesmo.
        let mine = parse_ps(
            " 4732 1 4700 /Applications/Claude.app/claude --resume=f105a514-0126-4ea2-9d49-538fe80e9e28\n",
        );
        assert!(holders_in(&mine, "f105a514-0126-4ea2-9d49-538fe80e9e28").is_empty());
    }

    #[test]
    fn another_conversations_run_is_left_alone() {
        let rows = parse_ps(REAL);
        assert!(holders_in(&rows, "36f0d281-05f1-43e8-81bb-e5f1d8d64fbe").is_empty());
    }

    /// O grupo do resto é o da própria Relay (2250) porque a 0.3.15 ainda não
    /// criava grupo. Levá-lo matava a aplicação — e é por isso que o
    /// `group_is_ours` existe em vez de se confiar no pgid.
    #[test]
    fn a_pre_0_3_16_group_is_never_taken_whole() {
        let rows = parse_ps(REAL);
        assert!(!group_is_ours(&rows, 2250), "2250 é o grupo da Relay");
        let held = holders_in(&rows, "f105a514-0126-4ea2-9d49-538fe80e9e28");
        // Em vez do grupo, o CLI e o sidecar que o levantou — e mais nada.
        let doomed: Vec<u32> = kin(&rows, held[0]).iter().map(|r| r.pid).collect();
        assert_eq!(doomed, vec![14791, 14786]);
        assert!(!doomed.contains(&25827), "a Relay nunca");
        assert!(!doomed.contains(&4732), "o Claude da operadora nunca");
    }

    /// E num sidecar da 0.3.16, onde o líder do grupo é ele próprio, leva-se o
    /// grupo — que é o que apanha o CLI mesmo que a árvore tenha crescido.
    #[test]
    fn a_group_we_created_is_taken_whole() {
        let rows = parse_ps(
            "\
900 800 800 /x/sidecar/node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64/claude --resume=abc
800   1 800 node /x/sidecar/index.mjs
",
        );
        assert!(group_is_ours(&rows, 800));
    }

    /// A armadilha do arranque. Uma segunda Relay aberta ao mesmo tempo tem
    /// turnos a correr; varrer "tudo o que é nosso" matava-lhos a meio. Só se
    /// leva o que já não tem pai.
    #[test]
    fn a_second_relays_live_run_is_not_swept_on_start() {
        let rows = parse_ps(
            "\
900 800  800 /x/sidecar/node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64/claude --resume=live
800 777  800 node /x/sidecar/index.mjs
777   1  777 /Applications/Relay.app/Contents/MacOS/relay
910 810  810 /x/sidecar/node_modules/@anthropic-ai/claude-agent-sdk-darwin-arm64/claude --resume=abandoned
810   1  810 node /x/sidecar/index.mjs
",
        );
        let orphans = orphaned_sidecars(&rows);
        assert_eq!(orphans.len(), 1, "só o sidecar sem pai");
        assert_eq!(orphans[0].pid, 810);
        // E leva o CLI que levantou, e só esse.
        let doomed: Vec<u32> = brood(&rows, orphans[0]).iter().map(|r| r.pid).collect();
        assert_eq!(doomed, vec![910, 810]);
        assert!(!doomed.contains(&900), "o turno vivo da outra Relay nunca");
    }

    /// E o resto verdadeiro do #108 é apanhado por este critério: o sidecar
    /// dele tinha mesmo `ppid` 1.
    #[test]
    fn the_real_stray_is_caught_on_start() {
        let rows = parse_ps(REAL);
        let orphans = orphaned_sidecars(&rows);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].pid, 14786);
        let doomed: Vec<u32> = brood(&rows, orphans[0]).iter().map(|r| r.pid).collect();
        assert!(doomed.contains(&14791) && doomed.contains(&14786));
        assert!(!doomed.contains(&25827) && !doomed.contains(&4732));
    }

    /// O engano que o teste de processos a sério apanhou: um id que é prefixo
    /// de outro. Com `contains` isto matava a sessão errada.
    #[test]
    fn a_session_id_that_prefixes_another_is_not_a_match() {
        let rows = parse_ps(
            "900 800 800 /x/node_modules/@anthropic-ai/claude-agent-sdk-x/claude --resume=abc-longa --verbose\n",
        );
        assert!(holders_in(&rows, "abc-longa").len() == 1);
        assert!(holders_in(&rows, "abc").is_empty(), "prefixo não é a mesma sessão");
        assert!(holders_in(&rows, "abc-long").is_empty());
    }

    /// E o id no fim da linha, sem nada a seguir, continua a casar.
    #[test]
    fn the_flag_still_matches_at_the_end_of_the_line() {
        let rows = parse_ps("900 800 800 /x/claude-agent-sdk-x/claude --resume=so-esta\n");
        assert_eq!(holders_in(&rows, "so-esta").len(), 1);
    }

    #[test]
    fn a_command_with_spaces_survives_the_parser() {
        let rows = parse_ps(REAL);
        assert!(rows[0].command.contains("Application Support"));
        assert!(rows[0].command.contains("--permission-mode manual"));
    }

    #[test]
    fn junk_lines_are_dropped_rather_than_guessed() {
        assert!(parse_ps("not a row\n\n  \n").is_empty());
        assert!(parse_ps("1 2 3\n").is_empty(), "sem comando não há linha");
    }
}
