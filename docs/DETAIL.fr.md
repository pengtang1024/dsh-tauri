# dsh-tauri — Guide détaillé

> Ce document est une compilation fidèle basée sur `README.md` et `docs/DEVELOPMENT.md` de ce dépôt, pour comprendre rapidement la position du projet, son architecture, ses dépendances et son installation, sa configuration, son utilisation, les commandes de vérification et les limitations / frontières connues. Les blocs de code, identifiants, nombres, chemins et commandes sont conservés verbatim.

## 1. Position du projet

Un client de bureau pour [DeepSeek Harness](https://github.com/deepseek-ai) construit avec **Tauri 2** : la fenêtre, la barre d'état système, le menu et les capacités natives proviennent de Rust + la WebView du système, tandis que Harness lui-même exécute le paquet npm officiel `@deepseek-ai/dsh`, c'est-à-dire le service Node pur derrière `dsh web`.

**Ni Electron ni DSH Desktop n'ont besoin d'être installés.** Un `node` et une copie de `node_modules/@deepseek-ai/dsh` constituent tout l'environnement d'exécution.

### Objectif principal

**Objectif :** Linux est la priorité, avec un support approfondi —toutes distributions et versions confondues—, dans le but d'obtenir la meilleure compatibilité, les meilleures performances et la consommation de ressources la plus faible ; c'est la seule plate-forme entièrement adaptée et vérifiée. Windows et macOS bénéficient d'un support de base sur ordinateur : compilation et exécution suffisent, sans adaptation ni optimisation approfondies. Les utilisateurs de Windows / macOS sont invités à télécharger directement la version bureau officielle de DeepSeek Harness depuis le [dépôt GitHub officiel](https://github.com/deepseek-ai/deepseek-harness) ou d'autres canaux officiels ; l'interface Tauri de ce projet n'est pas l'option privilégiée sous Win/Mac. Les plates-formes mobiles (iOS / Android) ne sont pas prises en charge : l'architecture repose sur un service Node local associé à la WebView du système, ce qui ne convient pas aux restrictions de bac à sable et d'arrière-plan du mobile.

> **État actuel (tel quel) :** Linux est la seule plate-forme entièrement adaptée et vérifiée (le code repose sur les commandes natives Linux `zenity` / `wl-clipboard` / `notify-send` et n'a été vérifié que sous Linux + WebKitGTK 2.52 ; la compatibilité multi-distributions reste un objectif, et non un fait entièrement vérifié). Windows et macOS offrent un support de base : ils compilent et s'exécutent, mais sans adaptation ni optimisation approfondies et sans vérification exhaustive. Les plates-formes mobiles (iOS / Android) sont explicitement non prises en charge, car l'architecture repose sur un service Node local associé à la WebView du système, ce qui ne convient pas aux restrictions de bac à sable et d'arrière-plan du mobile.

### Quelle est la différence avec « fourrer une application Electron dans Tauri » ?

Tauri fournit uniquement la WebView du système et un backend Rust ; il n'embarque pas d'environnement d'exécution Node. La clé n'est donc pas de changer d'enveloppe, mais **quel noyau est exécuté** :

| Approche | Environnement d'exécution | Résultat |
| --- | --- | --- |
| Tauri enveloppant le répertoire d'installation d'Electron | app.asar de `/opt/DSH NEXT` (le noyau dépend du processus principal Electron) | Vous devez toujours rétro-ingénier l'installation officielle, et cela casse à chaque mise à jour en amont |
| **Ce projet** | `@deepseek-ai/dsh` de npm, `dsh web` comme simple processus Node | Le noyau est un paquet distribué publiquement ; la mise à jour ne nécessite qu'un changement de numéro de version |

Tout ce qu'il reste à faire à l'enveloppe est trois choses : démarrer le service, pointer la fenêtre vers lui et fournir les capacités qu'un navigateur ne peut pas offrir.

## 2. Architecture

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

Le script d'injection (`src-tauri/src/native.rs::SHIM`) s'exécute avant les scripts de la page, ajoute `__DSH_DIRECTORY_PICKER__` et `__DSH_TAURI__` (presse-papiers / notifications / liens externes) et synchronise `document.title` avec la barre de titre native — WebKit ne le fait pas de lui-même.

### Couches

| Couche | Emplacement | Responsabilité |
| --- | --- | --- |
| Supervision du service | `src-tauri/src/supervisor.rs` | lancer `dsh web`, analyser l'URL depuis stdout, redémarrer après plantage, sortie par SIGTERM |
| Capacités natives | `src-tauri/src/native.rs` | sélecteur de répertoire, presse-papiers, notifications, liens externes, titre de fenêtre + script d'injection |
| Enveloppe | `src-tauri/src/shell_ui.rs` | fenêtre, menu, barre d'état système, état de la fenêtre, page d'erreur, découpage géométrique |
| Instance unique | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### Structure des répertoires

```
src-tauri/src/main.rs      Point d'entrée : builder Tauri, enregistrement des commandes, flux de démarrage et événements d'arrêt
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

### Correspondance des capacités

| Capacité | Implémentation |
| --- | --- |
| Sessions / espaces de travail / plugins | Web UI officielle, chargée depuis `dsh web` |
| Sélecteur de répertoire, presse-papiers, notifications, liens externes | Script d'injection + commandes Tauri |
| Titre de fenêtre, taille, barre d'état système, instance unique | `shell_ui.rs` / `single_instance.rs` |
| Redémarrage automatique après plantage | `supervisor.rs`, page d'erreur après 3 tentatives au maximum |
| Installation / mise à jour de l'environnement d'exécution | Automatique (installation au premier lancement, vérification des mises à jour après le lancement) |
| Progression du démarrage | splash interroge `runtime_status` : étape + temps écoulé |

## 3. Dépendances et installation

- Node 24+ (dans le PATH, ou spécifié via `DSH_TAURI_NODE`)
- Rust 1.9x, `webkit2gtk-4.1`, `libsoup-3.0`, `librsvg2`, `libayatana-appindicator3`, `patchelf`
- Outils GNOME à l'exécution : `zenity` (sélecteur de répertoire), `wl-clipboard` (presse-papiers), `libnotify-bin` (notifications)

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin
```

## 4. Construction et utilisation

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` démarre le service lui-même et l'ouvre dans une fenêtre.

L'empaquetage (deb / AppImage) utilise `npx tauri build` et ne distribue pas `node_modules` avec le paquet : **le premier lancement installe automatiquement**, en plaçant `@deepseek-ai/dsh` dans `~/.dsh/dsh-tauri/runtime`. Pendant ce temps, splash affiche l'étape (recherche / installation / démarrage) et le temps écoulé ; vous n'avez aucune commande à exécuter au préalable.

L'enveloppe cherche dans l'ordre « `DSH_TAURI_RUNTIME` → `runtime/` dans le dépôt → `~/.dsh/dsh-tauri/runtime` ». Ce n'est qu'en mode **hors ligne** qu'il faut installer une fois manuellement :

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### Mises à jour automatiques

Une fois le service lancé, l'enveloppe démarre un thread en arrière-plan qui exécute `npm view @deepseek-ai/dsh version`. Si la version en amont est plus récente que la version locale, elle l'installe automatiquement dans le même répertoire d'environnement d'exécution, puis utilise `notify-send` pour afficher « 已更新到 <版本>，重启生效 ».

- **N'interrompt pas la session en cours** : le service en cours d'exécution n'est pas tué ; l'ancien répertoire est renommé en `<runtime>.old-<pid>` et conservé pour qu'il continue à le lire, tandis que la nouvelle version n'est chargée qu'au **prochain lancement**
- **Échoue silencieusement et réessaie** : un échec de vérification ou d'installation n'écrit qu'une seule ligne de journal (`[runtime] 更新检查失败（下次启动再试）`), n'affiche aucune fenêtre contextuelle, n'affecte pas l'utilisation et est retenté au prochain lancement
- **Ne met à jour que les répertoires qu'il a installés lui-même** : le répertoire spécifié par `DSH_TAURI_RUNTIME` et le `runtime/` du dépôt appartiennent à l'utilisateur ou à git ; l'enveloppe ne fait que journaliser et n'écrit jamais dedans

La mise à jour manuelle reste possible ; elle équivaut à ce que l'enveloppe ne fasse rien :

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### Ordre de recherche de l'environnement d'exécution

`DSH_TAURI_RUNTIME` → `runtime/` dans le dépôt (ne compte que si `node_modules` existe) → `~/.dsh/dsh-tauri/runtime` → `runtime/` sous le répertoire de ressources empaqueté.

Si aucun n'est trouvé, une page d'erreur est affichée avec la commande d'installation dans son corps.

`runtime::Source` enregistre quel niveau a été trouvé. Seuls `Home` et `Resource` sont les répertoires propres à l'enveloppe et peuvent être écrits par les mises à jour automatiques ; `Explicit` (`DSH_TAURI_RUNTIME`) et `Checkout` (dans le dépôt) ne reçoivent qu'une ligne de journal et ne sont jamais touchés — ils peuvent contenir des fichiers suivis par git.

### Installation automatique au premier lancement

Lorsque `runtime::ensure` ne trouve aucun environnement d'exécution dans les quatre niveaux et n'est pas en mode hors ligne, il appelle `install::install(dir, "latest")`, qui installe dans `~/.dsh/dsh-tauri/runtime` :

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- Pendant l'installation, `progress::set(Stage::Installing, …)` est défini et splash interroge `runtime_status` pour afficher l'étape et le temps écoulé ;
- `exec.rs` exécute npm dans sa propre session et récupère tout le groupe de processus à l'expiration du délai ou à la sortie, sans laisser de processus petits-enfants de `postinstall` ;
- Tout échec renvoie une erreur lisible accompagnée de la commande manuelle, via `progress::fail` → page d'erreur.

### Implémentation de la mise à jour automatique

Après le démarrage du service, `update::spawn_once(dir, source)` lance un thread `dsh-update` (une seule fois par processus) :

1. `version_at(dir)` lit la version dans le `node_modules/@deepseek-ai/dsh/package.json` local ;
2. `npm view @deepseek-ai/dsh version` récupère la version en amont (délai de 90 secondes, en respectant la configuration du miroir npm de l'utilisateur) ;
3. `version::is_newer` compare (comparaison sémantique pure en Rust, y compris les règles de prerelease ; voir les tests unitaires de `version.rs`) ;
4. Si une mise à jour est disponible, `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` suit le même flux de staging + remplacement atomique → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`.

Contraintes : ne pas tuer le service en cours d'exécution (la nouvelle version n'est chargée qu'au prochain lancement), en cas d'échec n'imprimer que `[runtime] 更新检查失败（下次启动再试）` et réessayer au prochain lancement, et lorsque `Source::updatable()` est faux, n'écrire qu'une ligne de journal.

### Remplacement atomique et nettoyage

- Le répertoire de staging `<runtime>.new-<pid>` et l'ancien arbre `<runtime>.old-<pid>` sont des **répertoires frères** de l'environnement d'exécution, ce qui garantit que `rename` reste sur le même système de fichiers et soit atomique ;
- `commit` renomme d'abord l'ancien répertoire en `.old-<pid>`, puis renomme le staging à sa place ; si la deuxième étape échoue, l'ancien répertoire est renommé en sens inverse (le message d'erreur indique « 旧运行时已恢复 ») ;
- `Staging::drop` supprime l'arbre de staging lorsqu'il n'y a pas eu de commit — une installation interrompue ne laisse aucun produit à moitié terminé ;
- L'ancien arbre est **conservé pour le service encore en cours d'exécution** (ses modules ont été chargés depuis l'ancien arbre), si bien qu'après une mise à jour deux copies coexistent sur le disque (environ 1.2G) ; `swap::clean_stale_runtime()` est appelé dans le `.setup` de `main.rs`, après l'obtention du verrou d'instance unique, et nettoie tous les frères `<name>.new-*` / `<name>.old-*`, ne journalisant que les échecs.

## 5. Configuration et variables d'environnement

`cargo run` démarre le service lui-même et l'ouvre dans une fenêtre. Options de surcharge courantes :

| Variable | Rôle |
| --- | --- |
| `DSH_TAURI_RUNTIME` | répertoire de l'environnement d'exécution (contenant `node_modules`) |
| `DSH_TAURI_NODE` | exécutable node |
| `DSH_TAURI_HOST` | hôte auquel le service se lie, loopback uniquement (par défaut `127.0.0.1` ; `localhost` / `::1` autorisés) |
| `DSH_TAURI_NPM` | chemin de l'exécutable npm (npm du PATH par défaut ; utilisé par l'installation/la mise à jour automatiques et la détection de version) |
| `DSH_TAURI_PROFILE` | quel profil lancer (par défaut `web`) |
| `DSH_HOME` | répertoire de données (par défaut `~/.dsh`) |
| `DSH_TAURI_URL` | pour le développement : ouvrir directement un service loopback déjà en cours d'exécution (http(s) + 127.0.0.1/localhost/[::1] uniquement), sans en lancer un |

### Interrupteurs de mise à jour automatique (variables d'environnement)

| Variable | Rôle |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | désactive la vérification de version ; l'installation automatique au premier lancement continue |
| `DSH_TAURI_OFFLINE=1` | désactive toute activité réseau (installation + vérification) ; permet de démarrer hors ligne avec un environnement d'exécution existant |

### Les deux interrupteurs (valeurs et effets)

| Variable | Valeurs | Effet |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | désactive l'installation et la vérification (toute activité réseau) ; page d'erreur si aucun environnement d'exécution |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | désactive uniquement la vérification de version ; l'installation automatique au premier lancement continue |

L'implémentation se trouve dans `runtime.rs::offline()` / `auto_update_enabled()` ; `auto_update_enabled` court-circuite `OFFLINE` en premier, si bien qu'en mode hors ligne les deux sont désactivés.

## 6. Points clés de l'implémentation

1. **L'URL du service provient uniquement de stdout** : `dsh web` affiche `dsh web: http://127.0.0.1:<port>/?token=…` ; ne devinez pas le port vous-même — `--port 0` laisse le système en attribuer un.
2. **La fenêtre doit être créée directement sur l'URL après l'avoir obtenue**. Créer d'abord une fenêtre splash puis faire `navigate` amène WebKit à perdre le `Set-Cookie` de la redirection d'authentification (symptôme : la page n'affiche que `dsh web authentication required`). Utilisez donc une fenêtre splash avec un label distinct ; dès que l'URL est obtenue, créez une nouvelle fenêtre `main` puis détruisez le splash.
3. **Le script d'injection doit s'exécuter avant les scripts de la page** : `WebviewWindowBuilder::initialization_script`. Il repose sur `window.__TAURI__.core.invoke` fourni par `withGlobalTauri` (voir `tauri.conf.json`).
4. **Le token dans la chaîne de requête est un point d'entrée d'authentification à usage unique** : `GET` renvoie 303 et émet un cookie `dsh-auth-<authority>` (`HttpOnly; SameSite=Strict`). Le serveur génère le nom du cookie selon l'autorité : il change donc quand le port change.
5. **La sortie passe par SIGTERM** : `dsh::shutdown()` envoie d'abord `kill -TERM` et n'applique SIGKILL que si le processus n'est toujours pas sorti après 5 secondes, afin d'éviter de laisser des résidus WAL dans la base de données des sessions.
6. **La sortie depuis la barre d'état système** doit appeler `dsh::shutdown()` avant `app.exit(0)`, sinon des processus node orphelins restent en arrière-plan.

## 7. Commandes de vérification

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` nécessite que l'environnement d'exécution soit installé au préalable :

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### Débogage de l'installation et des mises à jour automatiques

**Faux npm** (sans réseau, sans télécharger 1.2G ; vérifie toute la chaîne installation / mise à jour) : `install::npm()` reconnaît `DSH_TAURI_NPM` ; il suffit d'écrire un script exécutable qui se fait passer pour npm.

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

- **Installation au premier lancement** : déplacez `~/.dsh/dsh-tauri/runtime` (`mv` vers `/tmp`), lancez `DSH_TAURI_NPM=/tmp/fake-npm cargo run` ; splash doit entrer dans l'étape « 正在安装 » et, une fois terminé, `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` doit exister ; le faux paquet n'a pas de vrai service, donc la fenêtre finira par s'arrêter sur un échec de démarrage — cela suffit pour vérifier la chaîne d'installation.
- **Chemin de mise à jour** : après l'installation, définissez `FAKE_LOCAL=1.0.0` et `FAKE_UPSTREAM=2.0.0` puis relancez ; le journal doit afficher `发现新版 2.0.0（本地 1.0.0）`, `已原子替换`, `旧运行时保留至下次启动清理`, et un frère `<runtime>.old-<pid>` apparaît ; un lancement supplémentaire doit afficher `清理上次运行留下的 …`.
- **Interrupteurs** : `DSH_TAURI_OFFLINE=1` sans environnement d'exécution → le corps de la page d'erreur contient la commande `npm install --prefix` ; `DSH_TAURI_AUTO_UPDATE=0` → le journal affiche `自动更新已关闭`, mais la première installation est tout de même effectuée.
- **Échec d'installation** : faites en sorte que la branche `install` du faux npm exécute `exit 1` → la page d'erreur contient la sortie de npm et la commande manuelle, et l'environnement d'exécution d'origine n'est pas affecté (`<runtime>.new-<pid>` a été supprimé par `Drop`).
- **Nettoyage final** : `pkill -x dsh-tauri` ; vérifiez qu'il ne reste aucun résidu `.new-*` / `.old-*` sous `~/.dsh/dsh-tauri/` et n'oubliez pas de supprimer / remettre en place le faux npm et l'environnement d'exécution déplacé.

### Vérification rapide (manuelle)

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

La fenêtre doit afficher : liste des sessions, espaces de travail, sélecteur de modèle et flux de sessions en cours.

## 8. Vérifié (Linux + WebKitGTK 2.52)

- `dsh web` est lancé par l'enveloppe et `http://127.0.0.1:<port>/?token=…` est analysé depuis stdout ; le premier écran apparaît en 3–30 secondes (selon le cache) ;
- Chaîne d'authentification : URL avec token → 303 + cookie `dsh-auth-*` → `GET /` renvoie le HTML du client (`tools/probe-web.mjs`) ;
- La fenêtre contient la Web UI complète : liste des sessions, espaces de travail, sélection du modèle, pages de plugins et flux de sessions en cours sont présents ;
- La fenêtre se renomme selon `document.title` ; à la fermeture, la préférence « fermer vers la barre d'état système » ou « quitter » est respectée (la préférence est le champ `closeToTray` dans `~/.dsh/desktop-preferences.json` : `true` = fermer vers la barre d'état système, `false` = quitter, par défaut `true`) ; la taille est restaurée au redémarrage ;
- Un deuxième lancement donne le focus à la fenêtre existante puis quitte, sans démarrer un second service ;
- Le service redémarre automatiquement après avoir été tué (3 fois au maximum) ; s'il échoue encore, une page d'erreur lisible est affichée ;
- Au premier lancement sans environnement d'exécution, `npm install` s'exécute automatiquement dans `~/.dsh/dsh-tauri/runtime`, et splash affiche l'étape en cours et le temps écoulé ;
- Le service démarre automatiquement après l'installation ; lorsqu'une version en amont plus récente existe, elle est installée en arrière-plan et `notify-send` affiche « 重启生效 », sans affecter le service en cours d'exécution ;
- Avec `DSH_TAURI_OFFLINE=1` et aucun environnement d'exécution local, une page d'erreur avec la commande manuelle est affichée au lieu d'une fenêtre blanche ;
- Les mises à jour passent par un répertoire de staging `<runtime>.new-<pid>` + remplacement atomique : une installation interrompue n'endommage pas l'environnement d'exécution déjà installé.

## 9. Limitations connues / frontières

| Élément | État actuel |
| --- | --- |
| Vue intégrée de la page de la plateforme (utilisation / recharge) | Ouverte dans le navigateur ; aucun intégration native n'est implémentée |
| L'ancien environnement d'exécution occupe le disque après la mise à jour | L'ancien arbre est renommé `<runtime>.old-<pid>` et conservé pour le service encore en cours d'exécution, **nettoyé seulement au prochain lancement** ; pendant ce temps il occupe environ une copie supplémentaire de l'environnement d'exécution (environ 1.2G) |
| Résidus après SIGKILL | Lorsque l'enveloppe reçoit `SIGKILL`, elle n'a pas le temps de récupérer les processus enfants, si bien que les processus petits-enfants `postinstall` de npm peuvent brièvement subsister ; une sortie normale (SIGTERM) n'est pas affectée |
| Windows / macOS / mobiles (iOS / Android) | **Windows / macOS : support de base** — compilent et s'exécutent, sans adaptation ni optimisation approfondies et sans vérification exhaustive ; les utilisateurs de Windows / macOS sont invités à télécharger la version bureau officielle depuis les canaux officiels ; l'interface Tauri de ce projet n'est pas le premier choix ; **mobiles (iOS / Android) : non pris en charge**, car l'architecture repose sur un service Node local + WebView du système, ce qui ne convient pas aux restrictions de bac à sable et d'arrière-plan du mobile ; seul Linux est entièrement adapté et vérifié |
| Position de la fenêtre sous Wayland | Déterminée par le compositeur ; `position()` n'affecte que les compositeurs de style X11 |
| Attente au premier lancement | L'étape et le temps écoulé sont visibles, mais il n'y a pas de sortie npm ligne par ligne |

## 10. Remarques

Ce projet n'est pas officiellement lié à DeepSeek et ne contient aucun binaire officiel ; l'environnement d'exécution est installé par l'utilisateur avec les commandes ci-dessus. Le code est publié sous licence MIT, voir [LICENSE](../LICENSE).
