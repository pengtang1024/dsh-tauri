# dsh-tauri — Guia detalhado

> Este documento é uma compilação fiel baseada no `README.md` e `docs/DEVELOPMENT.md` deste repositório, para uma compreensão rápida do posicionamento do projeto, da arquitetura, das dependências e da instalação, da configuração, do uso, dos comandos de verificação e das limitações / fronteiras conhecidas. Blocos de código, identificadores, números, caminhos e comandos são mantidos literalmente.

## 1. Posicionamento do projeto

Um cliente desktop para o [DeepSeek Harness](https://github.com/deepseek-ai) construído com **Tauri 2**: a janela, a bandeja, o menu e as capacidades nativas vêm do Rust + da WebView do sistema, enquanto o próprio Harness executa o pacote npm oficial `@deepseek-ai/dsh`, ou seja, o serviço Node puro por trás de `dsh web`.

**Não é necessário instalar o Electron nem o DSH Desktop.** Um `node` e uma cópia de `node_modules/@deepseek-ai/dsh` constituem todo o ambiente de execução.

### Objetivo principal

**Objetivo:** o Linux é a prioridade, com suporte profundo — em todas as distribuições e versões —, com o objetivo de buscar a melhor compatibilidade, desempenho e o menor consumo de recursos; é a única plataforma totalmente adaptada e verificada. Windows e macOS contam com suporte básico para desktop: basta compilar e executar, sem adaptação ou otimização profunda. Recomenda-se que os usuários de Windows / macOS baixem diretamente a versão desktop oficial do DeepSeek Harness no [repositório oficial no GitHub](https://github.com/deepseek-ai/deepseek-harness) ou por outros canais oficiais; a casca Tauri deste projeto não é a opção preferida no Win/Mac. As plataformas móveis (iOS / Android) não são suportadas: a arquitetura depende de um serviço Node local somado à WebView do sistema, o que não se encaixa nas restrições de sandbox e de segundo plano do mobile.

> **Estado atual (conforme é):** o Linux é a única plataforma totalmente adaptada e verificada (o código depende de comandos nativos do Linux — `zenity` / `wl-clipboard` / `notify-send` — e foi verificado apenas em Linux + WebKitGTK 2.52; a compatibilidade com diversas distribuições continua sendo um objetivo, não um fato plenamente verificado). Windows e macOS oferecem suporte básico: compilam e executam, mas sem adaptação ou otimização profunda e sem verificação abrangente. As plataformas móveis (iOS / Android) não são suportadas de forma explícita, porque a arquitetura depende de um serviço Node local somado à WebView do sistema, o que não se encaixa nas restrições de sandbox e de segundo plano do mobile.

### Em que isso difere de «colocar um aplicativo Electron dentro do Tauri»?

O Tauri fornece apenas a WebView do sistema e um backend em Rust; ele não inclui um ambiente de execução Node. Portanto, o ponto-chave não é trocar a casca, mas **qual núcleo é executado**:

| Abordagem | Ambiente de execução | Resultado |
| --- | --- | --- |
| Tauri envolvendo o diretório de instalação do Electron | app.asar de `/opt/DSH NEXT` (o núcleo depende do processo principal do Electron) | Você ainda precisa fazer engenharia reversa da instalação oficial, e ela quebra a cada atualização upstream |
| **Este projeto** | `@deepseek-ai/dsh` do npm, `dsh web` como um processo Node comum | O núcleo é um pacote distribuído publicamente; atualizar exige apenas alterar o número da versão |

Tudo o que resta à casca fazer são três coisas: iniciar o serviço, apontar a janela para ele e fornecer as capacidades que um navegador não pode oferecer.

## 2. Arquitetura

```
┌────────────────────── Tauri 进程（Rust） ──────────────────────┐
│ 托盘 / 菜单 / 窗口状态 / 单实例 / 崩溃重启                      │
│ 原生命令：目录选择、剪贴板、通知、外链、窗口标题                │
│ window ──► http://127.0.0.1:<port>/?token=…                    │
└──────────┬───────────────────────────────────────▲─────────────┘
           │ spawn（stdout 里解析 URL）             │ invoke
┌──────────▼───────────────────────────────────────┴─────────────┐
│ node runtime/node_modules/@deepseek-ai/dsh/lib/bin.js           │
│   --profile web --host 127.0.0.1 --port 0 --no-open             │
│ 官方 Web UI + 插件 + 会话存储（DSH_HOME，默认 ~/.dsh）           │
└─────────────────────────────────────────────────────────────────┘
```

O script de injeção (`src-tauri/src/native.rs::SHIM`) é executado antes dos scripts da página, adiciona `__DSH_DIRECTORY_PICKER__` e `__DSH_TAURI__` (área de transferência / notificações / links externos) e sincroniza `document.title` com a barra de título nativa — o WebKit não faz isso por conta própria.

### Camadas

| Camada | Localização | Responsabilidade |
| --- | --- | --- |
| Supervisão do serviço | `src-tauri/src/supervisor.rs` | iniciar `dsh web`, analisar a URL a partir do stdout, reiniciar após falha, saída via SIGTERM |
| Capacidades nativas | `src-tauri/src/native.rs` | seletor de diretório, área de transferência, notificações, links externos, título da janela + script de injeção |
| Casca | `src-tauri/src/shell_ui.rs` | janela, menu, bandeja, estado da janela, página de erro, recorte de geometria |
| Instância única | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### Estrutura de diretórios

```
src-tauri/src/main.rs      Ponto de entrada: builder do Tauri, registro de comandos, fluxo de inicialização e eventos de desligamento
src-tauri/src/dsh.rs        运行时配置解析与信息上报
src-tauri/src/runtime.rs    运行时定位（Env → checkout → 安装目录）+ 两个开关
src-tauri/src/install.rs    npm 安装到暂存目录、带超时与失败信息
src-tauri/src/update.rs     启动后后台 `npm view` 比对版本、自动装新、notify-send
src-tauri/src/swap.rs       `.new-<pid>` 暂存 + 原子 rename，启动时清理 `.old-*`
src-tauri/src/progress.rs   启动阶段与已用时（splash 轮询）
src-tauri/src/exec.rs       带超时的子进程执行（整组进程一起收）
src-tauri/src/version.rs    语义化版本比较
src-tauri/src/supervisor.rs 监督线程：定位 → 安装 → 启动
src-tauri/src/banner.rs     从服务日志里解析 URL
src-tauri/src/native.rs     原生命令 + 注入脚本
src-tauri/src/shell_ui.rs   窗口、菜单、托盘、splash、错误页
src-tauri/src/single_instance.rs  单实例（Unix socket）
runtime/                    仅开发用：本地安装的 @deepseek-ai/dsh
tools/probe-web.mjs         运行时契约探针
docs/DEVELOPMENT.md         开发与验证手册
```

### Correspondência de capacidades

| Capacidade | Implementação |
| --- | --- |
| Sessões / espaços de trabalho / plugins | Web UI oficial, carregada de `dsh web` |
| Seletor de diretório, área de transferência, notificações, links externos | Script de injeção + comandos Tauri |
| Título da janela, tamanho, bandeja, instância única | `shell_ui.rs` / `single_instance.rs` |
| Reinício automático após falha | `supervisor.rs`, página de erro após no máximo 3 tentativas |
| Instalação / atualização do ambiente de execução | Automática (instalação no primeiro lançamento, verificação de atualizações após o lançamento) |
| Progresso da inicialização | splash consulta `runtime_status`: estágio + tempo decorrido |

## 3. Dependências e instalação

- Node 24+ (no PATH, ou especificado por `DSH_TAURI_NODE`)
- Rust 1.9x, `webkit2gtk-4.1`, `libsoup-3.0`, `librsvg2`, `libayatana-appindicator3`, `patchelf`
- Utilitários de execução: `zenity` (seletor de diretório / painel Sobre), `wl-clipboard` (área de transferência), `libnotify-bin` (notificações via `notify-send`), `xdg-utils` (links externos via `xdg-open`)
- O pacote `.deb` para Debian / Ubuntu declara os quatro utilitários em `bundle.linux.deb.depends`, portanto eles são instalados automaticamente e funcionam imediatamente; no AppImage ou na execução a partir do código-fonte, instale-os manualmente:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin xdg-utils
```

## 4. Compilação e uso

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

O `cargo run` inicia o serviço por conta própria e o abre em uma janela.

O empacotamento (deb / AppImage) usa `npx tauri build` e não distribui `node_modules` junto com o pacote: **o primeiro lançamento instala automaticamente**, colocando `@deepseek-ai/dsh` em `~/.dsh/dsh-tauri/runtime`. Durante isso, o splash mostra o estágio (busca / instalação / inicialização) e o tempo decorrido; você não precisa executar nenhum comando antes.

A casca procura na ordem «`DSH_TAURI_RUNTIME` → `runtime/` no repositório → `~/.dsh/dsh-tauri/runtime`». Somente no modo **offline** é necessário instalar manualmente uma vez:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### Atualizações automáticas

Assim que o serviço está no ar, a casca inicia uma thread em segundo plano que executa `npm view @deepseek-ai/dsh version`. Se a versão upstream for mais recente que a local, ela é instalada automaticamente no mesmo diretório de ambiente de execução e, em seguida, `notify-send` exibe «已更新到 <版本>，重启生效».

- **Não interrompe a sessão atual**: o serviço em execução não é morto; o diretório antigo é renomeado para `<runtime>.old-<pid>` e mantido para que ele continue lendo dele, enquanto a versão nova só é carregada no **próximo lançamento**
- **Falha silenciosamente e tenta de novo**: uma verificação ou instalação que falha escreve apenas uma linha de log (`[runtime] 更新检查失败（下次启动再试）`), não mostra popup, não afeta o uso e é tentada novamente no próximo lançamento
- **Atualiza apenas diretórios que ela mesma instalou**: o diretório indicado por `DSH_TAURI_RUNTIME` e o `runtime/` do repositório pertencem ao usuário ou ao git; a casca apenas registra em log e nunca escreve neles

A atualização manual continua possível, equivalendo a fazer a casca não a realizar:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### Ordem de busca do ambiente de execução

`DSH_TAURI_RUNTIME` → `runtime/` no repositório (conta apenas se `node_modules` existir) → `~/.dsh/dsh-tauri/runtime` → `runtime/` sob o diretório de recursos empacotado.

Se nada for encontrado, uma página de erro é exibida com o comando de instalação no texto.

`runtime::Source` registra qual nível foi encontrado. Somente `Home` e `Resource` são diretórios próprios da casca e podem ser escritos por atualizações automáticas; `Explicit` (`DSH_TAURI_RUNTIME`) e `Checkout` (no repositório) recebem apenas uma linha de log e nunca são tocados — ali podem existir arquivos rastreados pelo git.

### Instalação automática no primeiro lançamento

Quando `runtime::ensure` não encontra um ambiente de execução em nenhum dos quatro níveis e não está no modo offline, ele chama `install::install(dir, "latest")`, instalando em `~/.dsh/dsh-tauri/runtime`:

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- Durante a instalação, `progress::set(Stage::Installing, …)` é definido e o splash consulta `runtime_status` para mostrar o estágio e o tempo decorrido;
- `exec.rs` executa o npm em sua própria sessão e recolhe todo o grupo de processos ao expirar o tempo limite ou ao sair, sem deixar processos-netos de `postinstall`;
- Qualquer falha retorna um erro legível mais o comando manual, passando por `progress::fail` → página de erro.

### Implementação da atualização automática

Após o serviço iniciar, `update::spawn_once(dir, source)` inicia uma thread `dsh-update` (executa apenas uma vez por processo):

1. `version_at(dir)` lê a versão do `node_modules/@deepseek-ai/dsh/package.json` local;
2. `npm view @deepseek-ai/dsh version` obtém a versão upstream (tempo limite de 90 segundos, respeitando a configuração de espelho npm do usuário);
3. `version::is_newer` compara (comparação semântica pura em Rust, incluindo as regras de prerelease; veja os testes unitários em `version.rs`);
4. Se houver atualização, `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` segue o mesmo fluxo de staging + substituição atômica → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`.

Restrições: não matar o serviço em execução (a versão nova só carrega no próximo lançamento), em caso de falha imprimir apenas `[runtime] 更新检查失败（下次启动再试）` e tentar novamente no próximo lançamento, e quando `Source::updatable()` é falso, escrever apenas uma linha de log.

### Substituição atômica e limpeza

- O diretório de staging `<runtime>.new-<pid>` e a árvore antiga `<runtime>.old-<pid>` são **diretórios irmãos** do ambiente de execução, garantindo que `rename` ocorra no mesmo sistema de arquivos e de forma atômica;
- `commit` primeiro renomeia o diretório antigo para `.old-<pid>` e depois move o staging para o lugar dele; se o segundo passo falhar, o diretório antigo é renomeado de volta (a mensagem de erro indica «旧运行时已恢复»);
- `Staging::drop` exclui a árvore de staging quando não houve commit — uma instalação interrompida não deixa um produto pela metade;
- A árvore antiga é **mantida para o serviço ainda em execução** (seus módulos foram carregados da árvore antiga), de modo que após uma atualização existem duas cópias simultâneas em disco (cerca de 1.2G); `swap::clean_stale_runtime()` é chamado no `.setup` de `main.rs`, depois que o bloqueio de instância única é obtido, e limpa todos os irmãos `<name>.new-*` / `<name>.old-*`, registrando apenas falhas.

## 5. Configuração e variáveis de ambiente

`cargo run` inicia o serviço por conta própria e o abre em uma janela. Sobrescritas comuns:

| Variável | Função |
| --- | --- |
| `DSH_TAURI_RUNTIME` | diretório do ambiente de execução (contendo `node_modules`) |
| `DSH_TAURI_NODE` | executável do node |
| `DSH_TAURI_HOST` | host ao qual o serviço se vincula, apenas loopback (padrão `127.0.0.1`; `localhost` / `::1` permitidos) |
| `DSH_TAURI_NPM` | caminho do executável do npm (por padrão o npm do PATH; usado pela instalação/atualização automáticas e pela detecção de versão) |
| `DSH_TAURI_PROFILE` | qual perfil iniciar (padrão `web`) |
| `DSH_HOME` | diretório de dados (padrão `~/.dsh`) |
| Arquivo de estado da janela | `~/.dsh/dsh-tauri-window-state.json`: guarda a geometria da janela (tamanho, posição, estado maximizado) para restauração no próximo início; independente de `DSH_HOME` |
| `DSH_TAURI_URL` | para desenvolvimento: abrir diretamente um serviço loopback já em execução (apenas http(s) + 127.0.0.1/localhost/[::1]), sem iniciar um |

### Interruptores de atualização automática (variáveis de ambiente)

| Variável | Função |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | desativa a verificação de versão; a instalação automática no primeiro lançamento continua |
| `DSH_TAURI_OFFLINE=1` | desativa toda atividade de rede (instalação + verificação); permite iniciar offline com um ambiente de execução existente |

### Os dois interruptores (valores e efeitos)

| Variável | Valores | Efeito |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | desativa instalação e verificação (toda atividade de rede); página de erro se não houver ambiente de execução |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | desativa apenas a verificação de versão; a instalação automática no primeiro lançamento continua |

A implementação está em `runtime.rs::offline()` / `auto_update_enabled()`; `auto_update_enabled` primeiro curto-circuita `OFFLINE`, portanto no modo offline ambos estão desativados.

## 6. Pontos importantes da implementação

1. **A URL do serviço vem apenas do stdout**: `dsh web` imprime `dsh web: http://127.0.0.1:<port>/?token=…`; não adivinhe a porta — `--port 0` deixa o sistema alocar uma.
2. **A janela deve ser criada diretamente na URL após obtê-la**. Criar primeiro uma janela splash e depois usar `navigate` faz o WebKit perder o `Set-Cookie` do redirecionamento de autenticação (sintoma: a página mostra apenas `dsh web authentication required`). Por isso, use uma janela splash com um label separado; ao encontrar a URL, crie uma nova janela `main` e destrua a splash.
3. **O script de injeção deve executar antes dos scripts da página**: `WebviewWindowBuilder::initialization_script`. Ele depende de `window.__TAURI__.core.invoke`, fornecido por `withGlobalTauri` (veja `tauri.conf.json`).
4. **O token na string de consulta é um ponto de entrada de autenticação de uso único**: `GET` retorna 303 e emite um cookie `dsh-auth-<authority>` (`HttpOnly; SameSite=Strict`). O servidor gera o nome do cookie a partir da autoridade, portanto ele muda quando a porta muda.
5. **A saída passa pelo SIGTERM**: `dsh::shutdown()` primeiro executa `kill -TERM` e só aplica SIGKILL se o processo ainda não tiver saído após 5 segundos, para evitar resíduos WAL no banco de dados de sessões.
6. **A saída pela bandeja** deve chamar `dsh::shutdown()` antes de `app.exit(0)`, senão processos node órfãos ficam para trás.

## 7. Comandos de verificação

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` requer que o ambiente de execução esteja instalado previamente:

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### Depuração da instalação e das atualizações automáticas

**npm falso** (sem rede, sem baixar 1.2G; verifica toda a cadeia de instalação / atualização): `install::npm()` reconhece `DSH_TAURI_NPM`; basta escrever um script executável que se passe por npm.

```bash
#!/usr/bin/env bash
# /tmp/fake-npm ：DSH_TAURI_NPM=/tmp/fake-npm
set -e
case "$1" in
  --version) echo "10.0.0" ;;
  view)      echo "${FAKE_UPSTREAM:-9.9.9}" ;;          # 假装上游版本
  install)                                              # npm install --prefix X …
    prefix=""; prev=""
    for a in "$@"; do [ "$prev" = "--prefix" ] && prefix="$a"; prev="$a"; done
    pkg="$prefix/node_modules/@deepseek-ai/dsh"
    mkdir -p "$pkg/lib"
    echo '{"name":"@deepseek-ai/dsh","version":"'"${FAKE_LOCAL:-1.0.0}"'"}' > "$pkg/package.json"
    : > "$pkg/lib/bin.js"
    ;;
esac
```

- **Instalação no primeiro lançamento**: mova `~/.dsh/dsh-tauri/runtime` para outro lugar (`mv` para `/tmp`), execute `DSH_TAURI_NPM=/tmp/fake-npm cargo run`; o splash deve entrar no estágio «正在安装» e, ao terminar, `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` deve existir; o pacote falso não tem um serviço real, então a janela acabará parando numa falha de inicialização — isso basta para verificar a cadeia de instalação.
- **Caminho de atualização**: após a instalação, defina `FAKE_LOCAL=1.0.0` e `FAKE_UPSTREAM=2.0.0` e inicie novamente; o log deve mostrar `发现新版 2.0.0（本地 1.0.0）`, `已原子替换`, `旧运行时保留至下次启动清理`, e um irmão `<runtime>.old-<pid>` aparece; um novo lançamento deve imprimir `清理上次运行留下的 …`.
- **Interruptores**: `DSH_TAURI_OFFLINE=1` sem ambiente de execução → o corpo da página de erro contém o comando `npm install --prefix`; `DSH_TAURI_AUTO_UPDATE=0` → o log mostra `自动更新已关闭`, mas a primeira instalação ainda é executada.
- **Falha de instalação**: faça o ramo `install` do npm falso executar `exit 1` → a página de erro traz a saída do npm e o comando manual, e o ambiente de execução original não é afetado (`<runtime>.new-<pid>` foi removido por `Drop`).
- **Limpeza final**: `pkill -x dsh-tauri`; confirme que não restam `.new-*` / `.old-*` em `~/.dsh/dsh-tauri/` e lembre-se de excluir / devolver o npm falso e o ambiente de execução movido.

### Verificação rápida (manual)

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

A janela deve mostrar: lista de sessões, espaços de trabalho, seletor de modelo e fluxos de sessão em execução.

## 8. Verificado (Linux + WebKitGTK 2.52)

- `dsh web` é iniciado pela casca e `http://127.0.0.1:<port>/?token=…` é analisado do stdout; a primeira tela aparece em 3–30 segundos (dependendo do cache);
- Cadeia de autenticação: URL com token → 303 + cookie `dsh-auth-*` → `GET /` retorna o HTML do cliente (`tools/probe-web.mjs`);
- A janela contém a Web UI completa: lista de sessões, espaços de trabalho, seleção de modelo, páginas de plugins e fluxos de sessão em andamento estão presentes;
- A janela renomeia-se conforme `document.title`; ao fechar, respeita a preferência «fechar para a bandeja» ou «sair» (a preferência é o campo `closeToTray` em `~/.dsh/desktop-preferences.json`: `true` = fechar para a bandeja, `false` = sair, por padrão `true`); o tamanho é restaurado ao reiniciar;
- Um segundo lançamento foca a janela existente e sai, sem iniciar um segundo serviço;
- O serviço reinicia automaticamente após ser morto (no máximo 3 vezes); se ainda falhar, uma página de erro legível é exibida;
- No primeiro lançamento sem ambiente de execução, `npm install` é executado automaticamente em `~/.dsh/dsh-tauri/runtime`, e o splash mostra o estágio atual e o tempo decorrido;
- O serviço inicia automaticamente após a instalação; quando existe uma versão upstream mais nova, ela é instalada em segundo plano e `notify-send` mostra «重启生效», sem afetar o serviço em execução;
- Com `DSH_TAURI_OFFLINE=1` e nenhum ambiente de execução local, uma página de erro com o comando manual é exibida em vez de uma janela em branco;
- As atualizações passam por um diretório de staging `<runtime>.new-<pid>` + substituição atômica, de modo que uma instalação interrompida não danifica o ambiente de execução já instalado.

## 9. Limitações conhecidas / fronteiras

| Item | Estado atual |
| --- | --- |
| Visualização incorporada da página da plataforma (uso / recarga) | Aberta no navegador; nenhuma incorporação nativa implementada |
| Ambiente de execução antigo ocupa disco após a atualização | A árvore antiga é renomeada `<runtime>.old-<pid>` e mantida para o serviço ainda em execução, **limpa apenas no próximo lançamento**; nesse período ocupa aproximadamente uma cópia extra do ambiente de execução (cerca de 1.2G) |
| Resíduos por SIGKILL | Quando a casca recebe `SIGKILL`, não tem tempo de recolher os processos filhos, então os processos-netos `postinstall` do npm podem permanecer brevemente; a saída normal (SIGTERM) não é afetada |
| Windows / macOS / móveis (iOS / Android) | **Windows / macOS: suporte básico** — compilam e executam, sem adaptação ou otimização profunda e sem verificação abrangente; recomenda-se que os usuários de Windows / macOS baixem a versão desktop oficial pelos canais oficiais; a casca Tauri deste projeto não é a primeira escolha; **móveis (iOS / Android): não suportados**, porque a arquitetura depende de um serviço Node local + WebView do sistema, o que não se encaixa nas restrições de sandbox e de segundo plano do mobile; apenas o Linux está totalmente adaptado e verificado |
| Posição da janela no Wayland | Determinada pelo compositor; `position()` afeta apenas compositores no estilo X11 |
| Espera no primeiro lançamento | O estágio e o tempo decorrido são visíveis, mas não há saída do npm linha a linha |

## 10. Observações

Este projeto não tem vínculo oficial com a DeepSeek e não contém binários oficiais; o ambiente de execução é instalado pelo usuário com os comandos acima. O código é publicado sob a licença MIT, veja [LICENSE](../LICENSE).
