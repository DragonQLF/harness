# Convenções

O que um agente que chega a este repositório precisa de saber antes de tocar em
nada. Não é a arquitectura — essa está no `docs/DECISIONS.md`, e é lá que se
regista o que muda. Isto é a lista curta das coisas que se aprendem por as
partir.

## Commits

**Nada de trailers `Co-Authored-By`.** Nem de assinaturas, nem de "gerado por".
A autoria de um commit é a linha `Author`, e chega. Este ficheiro existe porque
a convenção só estava escrita na ausência dela em sessenta commits, e uma
convenção que só se lê no `git log` é uma convenção que se perde.

As mensagens são em português, no tom do resto do repositório: o assunto diz o
que passou a ser verdade, o corpo diz porquê e o que se rejeitou. Um commit que
só diz *o quê* está a repetir o diff.

O `user.email` é local ao repositório de propósito — este é um repositório
público e o endereço de trabalho não tem nada que aparecer aqui. Confirmar antes
do primeiro commit de uma sessão, não depois do push.

## pnpm, nunca npm

O CI corre `pnpm install --frozen-lockfile`. Um `package-lock.json` ao lado do
`pnpm-lock.yaml` dessincroniza os dois e está ignorado por isso.

E há uma armadilha que só aparece no CI: o npm achata o `node_modules`, o pnpm
não. Um `import` de um pacote transitivo — que ninguém declarou no
`package.json` — resolve numa árvore instalada pelo npm e falha na do pnpm. A
v0.3.5 parou nas duas plataformas exactamente assim. **Verificar contra uma
árvore do pnpm**, não contra a que estiver lá.

## Antes de dizer que está feito

```
pnpm exec tsc --noEmit
pnpm run check:styles      # nenhum style={{}} feito só de literais
cargo test --workspace
pnpm run test:sidecar
```

Depois de mexer num tipo com `#[derive(TS)]`: `pnpm codegen`. Os ficheiros em
`src/lib/generated/` são gerados — nunca escritos à mão, e nunca corrigidos à
mão quando o gerador os reescreve.

Um processo que arranca não é um processo que fica de pé. Uma app que se fecha
sozinha ao fim de dois segundos não escreve erro nenhum, porque fechar-se
limpo é silencioso.

## O que não se inventa

Nenhum número no ecrã é decorativo. Onde o motor não tem resposta, o cartão
mostra o seu vazio e diz o que falta — não um valor plausível. Uma barra de
progresso sem tecto contra o qual medir fica vazia; um total que o backend não
manda fica num travessão. Se o dado devia existir, constrói-se o comando; não
se pinta o buraco.

A lógica vive em `crates/app`, `crates/domain` e `crates/engine`, onde o cargo
lhe pega. A caixa do Tauri é casca fina: recebe, chama, devolve.

Os comentários dizem *porquê*, não *o quê*, e são poucos. Os que estão em
português ficam em português.
