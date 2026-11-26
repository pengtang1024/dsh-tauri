# dsh-tauri — Detaillierte Anleitung

> Dieses Dokument ist eine getreue Zusammenstellung auf Grundlage von `README.md` und `docs/DEVELOPMENT.md` in diesem Repository, zum schnellen Verständnis der Ausrichtung des Projekts, der Architektur, der Abhängigkeiten und Installation, der Konfiguration, der Verwendung, der Verifizierungsbefehle sowie bekannter Einschränkungen / Grenzen. Codeblöcke, Bezeichner, Zahlen, Pfade und Befehle bleiben unverändert erhalten.

## 1. Ausrichtung des Projekts

Ein Desktop-Client für [DeepSeek Harness](https://github.com/deepseek-ai), gebaut mit **Tauri 2**: Fenster, Tray, Menü und native Fähigkeiten stammen aus Rust + der System-WebView, während Harness selbst das offizielle npm-Paket `@deepseek-ai/dsh` ausführt, also den reinen Node-Dienst hinter `dsh web`.

**Weder Electron noch DSH Desktop müssen installiert werden.** Ein `node` und eine Kopie von `node_modules/@deepseek-ai/dsh` sind die gesamte Laufzeitumgebung.

### Worin unterscheidet sich das vom „Hineinstopfen einer Electron-App in Tauri"?

Tauri stellt nur die System-WebView und ein Rust-Backend bereit; es liefert keine Node-Laufzeit mit. Der entscheidende Punkt ist daher nicht ein anderer Mantel, sondern **welcher Kern ausgeführt wird**:

| Ansatz | Laufzeitumgebung | Ergebnis |
| --- | --- | --- |
| Tauri um das Electron-Installationsverzeichnis | app.asar aus `/opt/DSH NEXT` (der Kern hängt vom Electron-Hauptprozess ab) | Sie müssen die offizielle Installation weiterhin reverse-engineeren, und sie bricht bei jedem Upgrade in der vorgelagerten Software zusammen |
| **Dieses Projekt** | `@deepseek-ai/dsh` von npm, `dsh web` als ganz normaler Node-Prozess | Der Kern ist ein öffentlich verteiltes Paket; ein Upgrade benötigt nur eine Änderung der Versionsnummer |

Für den Mantel bleiben nur noch drei Aufgaben übrig: den Dienst starten, das Fenster darauf richten und die Fähigkeiten bereitstellen, die ein Browser nicht bieten kann.

## 2. Architektur

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

Das Injektionsskript (`src-tauri/src/native.rs::SHIM`) läuft vor den Seitenskripten, ergänzt `__DSH_DIRECTORY_PICKER__` und `__DSH_TAURI__` (Zwischenablage / Benachrichtigungen / externe Links) und synchronisiert `document.title` mit der nativen Titelleiste — WebKit tut dies nicht von selbst.

### Schichten

| Schicht | Ort | Verantwortung |
| --- | --- | --- |
| Dienstüberwachung | `src-tauri/src/dsh.rs` | `dsh web` starten, URL aus stdout parsen, Neustart nach Absturz, Beenden mit SIGTERM |
| Native Fähigkeiten | `src-tauri/src/native.rs` | Verzeichnisauswahl, Zwischenablage, Benachrichtigungen, externe Links, Fenstertitel + Injektionsskript |
| Mantel | `src-tauri/src/shell_ui.rs` | Fenster, Menü, Tray, Fensterzustand, Fehlerseite, Geometrie-Zuschnitt |
| Einzelinstanz | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### Verzeichnisstruktur

```
src-tauri/src/dsh.rs        服务生命周期：spawn、解析 URL、崩溃重启、优雅退出
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

### Zuordnung der Fähigkeiten

| Fähigkeit | Implementierung |
| --- | --- |
| Sitzungen / Arbeitsbereiche / Plugins | Offizielle Web UI, geladen aus `dsh web` |
| Verzeichnisauswahl, Zwischenablage, Benachrichtigungen, externe Links | Injektionsskript + Tauri-Befehle |
| Fenstertitel, Größe, Tray, Einzelinstanz | `shell_ui.rs` / `single_instance.rs` |
| Automatischer Neustart nach Absturz | `dsh.rs`, Fehlerseite nach maximal 3 Versuchen |
| Installation / Update der Laufzeitumgebung | Automatisch (Installation beim ersten Start, Update-Prüfung nach dem Start) |
| Startfortschritt | splash fragt `runtime_status` ab: Phase + verstrichene Zeit |

## 3. Abhängigkeiten und Installation

- Node 24+ (im PATH oder angegeben über `DSH_TAURI_NODE`)
- Rust 1.9x, `webkit2gtk-4.1`, `libsoup-3.0`, `librsvg2`, `libayatana-appindicator3`, `patchelf`
- GNOME-Laufzeitwerkzeuge: `zenity` (Verzeichnisauswahl), `wl-clipboard` (Zwischenablage), `libnotify-bin` (Benachrichtigungen)

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin
```

## 4. Bauen und Verwendung

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` startet den Dienst selbst und öffnet ihn in einem Fenster.

Die Paketierung (deb / AppImage) verwendet `npx tauri build` und verteilt `node_modules` nicht mit dem Paket: **beim ersten Start wird automatisch installiert**, wobei `@deepseek-ai/dsh` nach `~/.dsh/dsh-tauri/runtime` installiert wird. Dabei zeigt splash die Phase (Suchen / Installieren / Starten) und die verstrichene Zeit an; Sie müssen zuvor keinen Befehl ausführen.

Der Mantel sucht in der Reihenfolge „`DSH_TAURI_RUNTIME` → `runtime/` im Repository → `~/.dsh/dsh-tauri/runtime`". Nur im **Offline**-Modus ist eine manuelle Installation einmalig nötig:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### Automatische Updates

Sobald der Dienst läuft, startet der Mantel einen Hintergrund-Thread mit `npm view @deepseek-ai/dsh version`. Ist die Upstream-Version neuer als die lokale, wird sie automatisch in dasselbe Laufzeitverzeichnis installiert und anschließend mit `notify-send` die Meldung „已更新到 <版本>，重启生效" angezeigt.

- **Unterbricht die aktuelle Sitzung nicht**: der laufende Dienst wird nicht getötet; das alte Verzeichnis wird in `<runtime>.old-<pid>` umbenannt und für ihn weiter vorgehalten, während die neue Version erst beim **nächsten Start** geladen wird
- **Schweigt bei Fehlern und versucht es erneut**: eine fehlgeschlagene Prüfung oder Installation schreibt nur eine Logzeile (`[runtime] 更新检查失败（下次启动再试）`), zeigt kein Popup, beeinträchtigt die Nutzung nicht und wird beim nächsten Start erneut versucht
- **Aktualisiert nur selbst installierte Verzeichnisse**: das über `DSH_TAURI_RUNTIME` angegebene Verzeichnis und das `runtime/` im Repository gehören dem Benutzer oder git; der Mantel schreibt nur eine Logzeile und greift nie ein

Ein manuelles Upgrade ist weiterhin möglich und entspricht davon, dass der Mantel es nicht tut:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### Reihenfolge der Laufzeit-Suche

`DSH_TAURI_RUNTIME` → `runtime/` im Repository (zählt nur, wenn `node_modules` existiert) → `~/.dsh/dsh-tauri/runtime` → `runtime/` unter dem gepackten Ressourcenverzeichnis.

Wird nichts gefunden, erscheint eine Fehlerseite mit dem Installationsbefehl im Text.

`runtime::Source` zeichnet auf, welche Stufe getroffen wurde. Nur `Home` und `Resource` sind eigene Verzeichnisse des Mantels und dürfen von automatischen Updates beschrieben werden; `Explicit` (`DSH_TAURI_RUNTIME`) und `Checkout` (im Repository) erhalten nur eine Logzeile und werden nie angefasst — dort können git-verfolgte Dateien liegen.

### Automatische Installation beim ersten Start

Findet `runtime::ensure` in keiner der vier Stufen eine Laufzeitumgebung und ist nicht im Offline-Modus, ruft es `install::install(dir, "latest")` auf und installiert nach `~/.dsh/dsh-tauri/runtime`:

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- Während der Installation wird `progress::set(Stage::Installing, …)` gesetzt und splash fragt `runtime_status` ab, um Phase und verstrichene Zeit anzuzeigen;
- `exec.rs` führt npm in einer eigenen Session aus und räumt bei Timeout oder Beendigung die gesamte Prozessgruppe ab, sodass keine `postinstall`-Enkelprozesse zurückbleiben;
- Jeder Fehler liefert eine lesbare Fehlermeldung plus den manuellen Befehl über `progress::fail` → Fehlerseite.

### Implementierung des automatischen Updates

Nach dem Start des Dienstes startet `update::spawn_once(dir, source)` einen Thread `dsh-update` (pro Prozess nur einmal):

1. `version_at(dir)` liest die Version aus dem lokalen `node_modules/@deepseek-ai/dsh/package.json`;
2. `npm view @deepseek-ai/dsh version` holt die Upstream-Version (90 Sekunden Timeout, unter Beachtung der npm-Spiegel-Konfiguration des Benutzers);
3. `version::is_newer` vergleicht (reiner Rust-Semver-Vergleich, einschließlich Prerelease-Regeln; siehe die Unit-Tests in `version.rs`);
4. Bei einem Update: `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` durchläuft denselben Ablauf aus Staging + atomarem Ersetzen → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`.

Einschränkungen: den laufenden Dienst nicht töten (die neue Version wird erst beim nächsten Start geladen), bei Fehlern nur `[runtime] 更新检查失败（下次启动再试）` ausgeben und beim nächsten Start erneut versuchen, und wenn `Source::updatable()` falsch ist, nur eine Logzeile schreiben.

### Atomares Ersetzen und Aufräumen

- Das Staging-Verzeichnis `<runtime>.new-<pid>` und der alte Baum `<runtime>.old-<pid>` sind **Geschwisterverzeichnisse** der Laufzeitumgebung, damit `rename` im selben Dateisystem und atomar bleibt;
- `commit` benennt zuerst das alte Verzeichnis in `.old-<pid>` um und verschiebt dann das Staging an seinen Platz; schlägt der zweite Schritt fehl, wird das alte Verzeichnis zurückbenannt (die Fehlermeldung besagt „旧运行时已恢复");
- `Staging::drop` löscht den Staging-Baum, wenn kein Commit stattfand — eine unterbrochene Installation hinterlässt kein halbfertiges Produkt;
- Der alte Baum wird **für den noch laufenden Dienst aufbewahrt** (seine Module wurden aus dem alten Baum geladen), sodass nach einem Update zwei Kopien gleichzeitig auf der Platte liegen (ca. 1.2G); `swap::clean_stale_runtime()` wird in `.setup` von `main.rs` aufgerufen, nachdem die Einzelinstanz-Sperre gewonnen wurde, und räumt alle Geschwister `<name>.new-*` / `<name>.old-*` auf, wobei nur Fehler protokolliert werden.

## 5. Konfiguration und Umgebungsvariablen

`cargo run` startet den Dienst selbst und öffnet ihn in einem Fenster. Übliche Überschreibungen:

| Variable | Wirkung |
| --- | --- |
| `DSH_TAURI_RUNTIME` | Verzeichnis der Laufzeitumgebung (enthält `node_modules`) |
| `DSH_TAURI_NODE` | node-Executable |
| `DSH_TAURI_HOST` | Host, an den der Dienst gebunden wird, nur Loopback (Standard `127.0.0.1`; `localhost` / `::1` erlaubt) |
| `DSH_TAURI_NPM` | Pfad zur npm-Executable (standardmäßig npm aus dem PATH; wird von automatischer Installation/Aktualisierung und Versionserkennung genutzt) |
| `DSH_TAURI_PROFILE` | welches Profil gestartet wird (Standard `web`) |
| `DSH_HOME` | Datenverzeichnis (Standard `~/.dsh`) |
| `DSH_TAURI_URL` | für die Entwicklung: einen bereits laufenden Loopback-Dienst direkt öffnen (nur http(s) + 127.0.0.1/localhost/[::1]), ohne einen zu starten |

### Schalter für automatische Updates (Umgebungsvariablen)

| Variable | Wirkung |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | Versionsprüfung abschalten; die automatische Installation beim ersten Start läuft weiter |
| `DSH_TAURI_OFFLINE=1` | alle Netzwerkaktivitäten abschalten (Installation + Prüfung); mit einer vorhandenen Laufzeitumgebung ist auch offline ein Start möglich |

### Die beiden Schalter (Werte und Wirkungen)

| Variable | Werte | Wirkung |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | Installation und Prüfung abschalten (alle Netzwerkaktivitäten); ohne Laufzeitumgebung Fehlerseite |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | nur die Versionsprüfung abschalten; die automatische Installation beim ersten Start läuft weiter |

Die Implementierung findet sich in `runtime.rs::offline()` / `auto_update_enabled()`; `auto_update_enabled` schaltet `OFFLINE` zuerst kurz, sodass im Offline-Modus beide aus sind.

## 6. Wichtige Implementierungshinweise

1. **Die Dienst-URL kommt ausschließlich aus stdout**: `dsh web` gibt `dsh web: http://127.0.0.1:<port>/?token=…` aus; raten Sie den Port nicht selbst — `--port 0` lässt das System einen vergeben.
2. **Das Fenster muss direkt auf der URL erstellt werden, nachdem sie erhalten wurde**. Zuerst ein splash-Fenster zu erstellen und dann `navigate` zu verwenden, lässt WebKit das `Set-Cookie` der Authentifizierungsweiterleitung verlieren (Symptom: die Seite zeigt nur `dsh web authentication required`). Deshalb ein splash-Fenster mit eigenem Label verwenden; wird die URL gefunden, ein neues `main`-Fenster erstellen und danach das splash zerstören.
3. **Das Injektionsskript muss vor den Seitenskripten laufen**: `WebviewWindowBuilder::initialization_script`. Es stützt sich auf `window.__TAURI__.core.invoke` aus `withGlobalTauri` (siehe `tauri.conf.json`).
4. **Das Token in der Query-Zeichenkette ist ein einmaliger Authentifizierungseintritt**: `GET` liefert 303 und setzt ein Cookie `dsh-auth-<authority>` (`HttpOnly; SameSite=Strict`). Der Server erzeugt den Cookie-Namen anhand der Autorität, daher ändert er sich bei einem Portwechsel.
5. **Das Beenden läuft über SIGTERM**: `dsh::shutdown()` sendet zuerst `kill -TERM` und erst nach 5 Sekunden ohne Beenden SIGKILL, um WAL-Reste in der Sitzungsdatenbank zu vermeiden.
6. **Tray-Beenden** muss `dsh::shutdown()` vor `app.exit(0)` aufrufen, sonst bleiben verwaiste node-Prozesse zurück.

## 7. Verifizierungsbefehle

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` setzt eine zuvor installierte Laufzeitumgebung voraus:

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### Debuggen der automatischen Installation und Updates

**Fake-npm** (kein Netzwerk, kein 1.2G-Download; prüft die gesamte Installations-/Update-Kette): `install::npm()` erkennt `DSH_TAURI_NPM`; es genügt, ein ausführbares Skript zu schreiben, das npm imitiert.

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

- **Installation beim ersten Start**: `~/.dsh/dsh-tauri/runtime` weglegen (`mv` nach `/tmp`), `DSH_TAURI_NPM=/tmp/fake-npm cargo run` ausführen; splash sollte die Phase „正在安装" erreichen und danach muss `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js` existieren; das Fake-Paket hat keinen echten Dienst, daher bleibt das Fenster am Ende bei einem Startfehler stehen — das genügt zur Verifikation der Installationskette.
- **Update-Pfad**: nach der Installation `FAKE_LOCAL=1.0.0` und `FAKE_UPSTREAM=2.0.0` setzen und erneut starten; im Log sollten `发现新版 2.0.0（本地 1.0.0）`, `已原子替换`, `旧运行时保留至下次启动清理` erscheinen, und ein Geschwisterverzeichnis `<runtime>.old-<pid>` entsteht; ein weiterer Start sollte `清理上次运行留下的 …` ausgeben.
- **Schalter**: `DSH_TAURI_OFFLINE=1` ohne Laufzeitumgebung → die Fehlerseite enthält im Text den Befehl `npm install --prefix`; `DSH_TAURI_AUTO_UPDATE=0` → das Log zeigt `自动更新已关闭`, aber die erste Installation wird trotzdem ausgeführt.
- **Installationsfehler**: den `install`-Zweig des Fake-npm `exit 1` ausführen lassen → die Fehlerseite enthält npm-Ausgabe und den manuellen Befehl, und die ursprüngliche Laufzeitumgebung bleibt unberührt (`<runtime>.new-<pid>` wurde durch `Drop` entfernt).
- **Aufräumen**: `pkill -x dsh-tauri`; prüfen, dass unter `~/.dsh/dsh-tauri/` keine `.new-*` / `.old-*`-Reste bleiben, und das Fake-npm sowie die weggelegte Laufzeitumgebung löschen / zurücklegen.

### Rauchtest (manuell)

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

Im Fenster sollten zu sehen sein: Sitzungsliste, Arbeitsbereiche, Modellauswahl und laufende Sitzungsströme.

## 8. Verifiziert (Linux + WebKitGTK 2.52)

- `dsh web` wird vom Mantel gestartet und `http://127.0.0.1:<port>/?token=…` aus stdout geparst; der erste Bildschirm erscheint innerhalb von 3–30 Sekunden (je nach Cache);
- Authentifizierungskette: Token-URL → 303 + `dsh-auth-*` Cookie → `GET /` liefert das Client-HTML (`tools/probe-web.mjs`);
- Im Fenster befindet sich die vollständige Web UI: Sitzungsliste, Arbeitsbereiche, Modellauswahl, Plugin-Seiten und laufende Sitzungsströme sind vorhanden;
- Das Fenster benennt sich entsprechend `document.title` um; beim Schließen gilt die Einstellung „zum Tray schließen" oder „beenden"; die Größe wird beim Neustart wiederhergestellt;
- Ein zweiter Start fokussiert das bestehende Fenster und beendet sich, ohne einen zweiten Dienst zu starten;
- Der Dienst startet nach dem Töten automatisch neu (maximal 3 Mal); bleibt er weiterhin fehlgeschlagen, wird eine lesbare Fehlerseite angezeigt;
- Beim ersten Start ohne Laufzeitumgebung führt `npm install` automatisch eine Installation nach `~/.dsh/dsh-tauri/runtime` durch, und splash zeigt die aktuelle Phase und die verstrichene Zeit;
- Der Dienst startet nach der Installation automatisch; gibt es eine neuere Upstream-Version, wird sie im Hintergrund installiert und `notify-send` zeigt „重启生效", ohne den laufenden Dienst zu beeinträchtigen;
- Mit `DSH_TAURI_OFFLINE=1` und ohne lokale Laufzeitumgebung erscheint eine Fehlerseite mit dem manuellen Befehl statt eines leeren Fensters;
- Updates laufen über ein Staging-Verzeichnis `<runtime>.new-<pid>` + atomares Ersetzen, sodass eine unterbrochene Installation die bereits installierte Laufzeitumgebung nicht beschädigt.

## 9. Bekannte Einschränkungen / Grenzen

| Punkt | Aktueller Stand |
| --- | --- |
| Eingebettete Ansicht der Plattformseite (Nutzung / Aufladen) | Wird im Browser geöffnet; keine native Einbettung implementiert |
| Alte Laufzeitumgebung belegt nach dem Update Speicher | Der alte Baum wird in `<runtime>.old-<pid>` umbenannt und für den noch laufenden Dienst aufbewahrt, **erst beim nächsten Start aufgeräumt**; in der Zwischenzeit belegt er etwa eine zusätzliche Kopie der Laufzeitumgebung (ca. 1.2G) |
| SIGKILL-Reste | Wird der Mantel mit `SIGKILL` beendet, bleibt keine Zeit, Kindprozesse abzuräumen, sodass npm-`postinstall`-Enkelprozesse kurzzeitig zurückbleiben können; ein normales Beenden (SIGTERM) ist nicht betroffen |
| Windows / macOS | Der Code ist gegen Linux-Native-Befehle geschrieben (zenity/wl-clipboard/notify-send); andere Plattformen erfordern Ersetzungen dieser Stellen |
| Fensterposition unter Wayland | Vom Compositor bestimmt; `position()` wirkt nur auf Compositoren im X11-Stil |
| Wartezeit beim ersten Start | Phase und verstrichene Zeit sind sichtbar, aber es gibt keine zeilenweise npm-Ausgabe |

## 10. Hinweise

Dieses Projekt steht in keinem offiziellen Zusammenhang mit DeepSeek und enthält keine offiziellen Binärdateien; die Laufzeitumgebung wird vom Benutzer mit den obigen Befehlen installiert. Der Code steht unter der MIT-Lizenz, siehe [LICENSE](../LICENSE).
