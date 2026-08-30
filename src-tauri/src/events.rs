//! Os nomes dos canais que a janela escuta.
//!
//! Eram treze literais espalhados por sete ficheiros, e o `engine://run` estava
//! escrito em quatro deles. Um canal é um contrato com o outro lado: quem o
//! escreve mal não parte nada de forma visível — o `emit` devolve `Ok`, o
//! `listen` fica calado, e o ecrã simplesmente não se mexe. Não há compilador
//! nem teste que apanhe uma letra trocada num sítio destes, e é por isso que
//! passam a ser constantes.
//!
//! O que está aqui é só o que o **backend emite**. O `menu://picked` fica no
//! `menu.rs` porque já lá estava com o mesmo argumento, e os dois canais do
//! splash (`splash://phase`, `splash://listening`) nunca passam por aqui: são
//! duas janelas do frontend a falar uma com a outra.

/// Um facto do quadro, com o seu número de sequência. Quem o ouve vai buscar o
/// snapshot em vez de replicar as regras do domínio (ver README).
pub const ENGINE_EVENT: &str = "engine://event";

/// Tudo o que um run diz enquanto corre, e também o que uma **conversa** diz:
/// a mesma forma com o id da conversa no lugar do cartão, que é o que deixa a
/// janela ter um escutador tipado só para os dois.
pub const ENGINE_RUN: &str = "engine://run";

/// Um pedido de permissão acabado de nascer, para a folha subir agora.
pub const APPROVAL_ASKED: &str = "approvals://asked";

/// A fila inteira, sempre que muda.
pub const APPROVAL_QUEUE: &str = "approvals://pending";

/// As verificações de um cartão, empurradas: quem as começou foi um run a
/// acabar, e o quadro não estava à espera delas.
pub const CARD_CHECKS: &str = "checks://card";

/// A lista de conversas, depois de o dono dela mudar alguma coisa.
pub const CONVERSATIONS: &str = "chat://conversations";

/// As propostas à espera do operador.
pub const INBOX: &str = "inbox://proposals";

/// Trabalho no repositório do Relay que nunca passou por um cartão.
pub const OUTSIDE_WORK: &str = "mirror://outside-work";

/// A saída da instalação das dependências do sidecar, linha a linha.
pub const SIDECAR_LOG: &str = "sidecar://log";

/// A janela vai fechar, e isto é o que se espera antes de ela ir.
pub const CLOSING_BEGAN: &str = "closing://began";

/// Em que passo do fecho vamos.
pub const CLOSING_PHASE: &str = "closing://phase";

/// O agente pediu para levar o operador a um sítio da app.
pub const NAVIGATE: &str = "ui://navigate";
