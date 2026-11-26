# dsh-tauri — Guía detallada

> Este documento es una recopilación fiel basada en `README.md` y `docs/DEVELOPMENT.md` de este repositorio, para comprender rápidamente la orientación del proyecto, la arquitectura, las dependencias y la instalación, la configuración, el uso, los comandos de verificación y las limitaciones/ fronteras conocidas. Los bloques de código, identificadores, números, rutas y comandos se mantienen literalmente.

## 1. Orientación del proyecto

Un cliente de escritorio para [DeepSeek Harness](https://github.com/deepseek-ai) construido con **Tauri 2**: la ventana, la bandeja, el menú y las capacidades nativas provienen de Rust + el WebView del sistema, mientras que el propio Harness ejecuta el paquete oficial de npm `@deepseek-ai/dsh`, es decir, el servicio puro de Node detrás de `dsh web`.

**No hace falta instalar Electron ni DSH Desktop.** Un `node` y una copia de `node_modules/@deepseek-ai/dsh` constituyen todo el entorno de ejecución.

### ¿En qué se diferencia de «meter una aplicación Electron dentro de Tauri»?

Tauri solo proporciona el WebView del sistema y un backend en Rust; no incluye un entorno de ejecución de Node. Por tanto, la clave no está en cambiar el envoltorio, sino en **qué núcleo se ejecuta**:

| Enfoque | Entorno de ejecución | Resultado |
| --- | --- | --- |
| Tauri envolviendo el directorio de instalación de Electron | app.asar de `/opt/DSH NEXT` (el núcleo depende del proceso principal de Electron) | Sigues teniendo que hacer ingeniería inversa de la instalación oficial, y se rompe cada vez que upstream actualiza |
| **Este proyecto** | `@deepseek-ai/dsh` de npm, `dsh web` como un proceso Node normal | El núcleo es un paquete distribuido públicamente; actualizar solo requiere cambiar el número de versión |

Lo único que le queda hacer al envoltorio es tres cosas: iniciar el servicio, apuntar la ventana hacia él y aportar las capacidades que un navegador no puede ofrecer.

## 2. Arquitectura

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

El script de inyección (`src-tauri/src/native.rs::SHIM`) se ejecuta antes de los scripts de la página, añade `__DSH_DIRECTORY_PICKER__` y `__DSH_TAURI__` (portapapeles / notificaciones / enlaces externos) y sincroniza `document.title` con la barra de título nativa: WebKit no lo hace por su cuenta.

### Capas

| Capa | Ubicación | Responsabilidad |
| --- | --- | --- |
| Supervisión del servicio | `src-tauri/src/supervisor.rs` | lanzar `dsh web`, analizar la URL desde stdout, reiniciar tras fallo, salida con SIGTERM |
| Capacidades nativas | `src-tauri/src/native.rs` | selector de directorio, portapapeles, notificaciones, enlaces externos, título de ventana + script de inyección |
| Envoltorio | `src-tauri/src/shell_ui.rs` | ventana, menú, bandeja, estado de ventana, página de error, recorte de geometría |
| Instancia única | `src-tauri/src/single_instance.rs` | `$XDG_RUNTIME_DIR/dsh-tauri.sock` |

### Estructura de directorios

```
src-tauri/src/main.rs      Punto de entrada: constructor de Tauri, registro de comandos, flujo de arranque y eventos de apagado
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

### Correspondencia de capacidades

| Capacidad | Implementación |
| --- | --- |
| Sesiones / espacios de trabajo / plugins | Web UI oficial, cargada desde `dsh web` |
| Selector de directorio, portapapeles, notificaciones, enlaces externos | Script de inyección + comandos Tauri |
| Título de ventana, tamaño, bandeja, instancia única | `shell_ui.rs` / `single_instance.rs` |
| Reinicio automático tras fallo | `supervisor.rs`, página de error tras un máximo de 3 intentos |
| Instalación/actualización del entorno de ejecución | Automática (instalación en el primer inicio, comprobación de actualizaciones tras el inicio) |
| Progreso de arranque | splash sondea `runtime_status`: fase + tiempo transcurrido |

## 3. Dependencias e instalación

- Node 24+ (en PATH, o especificado mediante `DSH_TAURI_NODE`)
- Rust 1.9x, `webkit2gtk-4.1`, `libsoup-3.0`, `librsvg2`, `libayatana-appindicator3`, `patchelf`
- Herramientas GNOME en ejecución: `zenity` (selector de directorio), `wl-clipboard` (portapapeles), `libnotify-bin` (notificaciones)

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf build-essential \
  zenity wl-clipboard libnotify-bin
```

## 4. Compilación y uso

```bash
npm install --prefix runtime @deepseek-ai/dsh   # 仅开发用：让 probe / 本仓库内启动拿到本体
npm test                                        # 运行时契约探针

cd src-tauri && cargo run
```

`cargo run` inicia el servicio por sí mismo y lo abre en una ventana.

El empaquetado (deb / AppImage) usa `npx tauri build` y no distribuye `node_modules` con el paquete: **el primer inicio instala automáticamente**, colocando `@deepseek-ai/dsh` en `~/.dsh/dsh-tauri/runtime`. Durante este proceso, splash muestra la fase (búsqueda / instalación / arranque) y el tiempo transcurrido; no necesitas ejecutar ningún comando antes.

El envoltorio busca en orden «`DSH_TAURI_RUNTIME` → `runtime/` dentro del repositorio → `~/.dsh/dsh-tauri/runtime`». Solo en **modo sin conexión** necesitas instalar una vez manualmente:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh
```

### Actualizaciones automáticas

Una vez levantado el servicio, el envoltorio inicia un hilo en segundo plano que ejecuta `npm view @deepseek-ai/dsh version`. Si la versión de upstream es más reciente que la local, la instala automáticamente en el mismo directorio de entorno de ejecución y después usa `notify-send` para mostrar «已更新到 <版本>，重启生效».

- **No interrumpe la sesión actual**: el servicio en ejecución no se mata; el directorio antiguo se renombra a `<runtime>.old-<pid>` y se conserva para que él siga leyéndolo, mientras que la versión nueva se carga solo en el **próximo inicio**
- **Falla en silencio y reintenta**: una comprobación o instalación fallida escribe solo una línea de registro (`[runtime] 更新检查失败（下次启动再试）`), no muestra ninguna ventana emergente, no afecta al uso y se reintenta en el próximo inicio
- **Solo actualiza los directorios que él mismo instaló**: el directorio especificado por `DSH_TAURI_RUNTIME` y el `runtime/` del repositorio pertenecen al usuario o a git; el envoltorio solo registra un mensaje y nunca escribe en ellos

La actualización manual sigue siendo posible; equivale a que el envoltorio no lo haga:

```bash
npm install --prefix ~/.dsh/dsh-tauri/runtime @deepseek-ai/dsh@latest
node tools/probe-web.mjs --runtime ~/.dsh/dsh-tauri/runtime   # 升级后先验契约
```

### Orden de búsqueda del entorno de ejecución

`DSH_TAURI_RUNTIME` → `runtime/` dentro del repositorio (solo cuenta si existe `node_modules`) → `~/.dsh/dsh-tauri/runtime` → `runtime/` bajo el directorio de recursos empaquetado.

Si no se encuentra ninguno, se muestra una página de error con el comando de instalación en el cuerpo.

`runtime::Source` registra qué nivel se encontró. Solo `Home` y `Resource` son directorios propios del envoltorio y pueden ser escritos por las actualizaciones automáticas; `Explicit` (`DSH_TAURI_RUNTIME`) y `Checkout` (dentro del repositorio) solo reciben una línea de registro y nunca se tocan: pueden contener archivos rastreados por git.

### Instalación automática en el primer inicio

Cuando `runtime::ensure` no encuentra un entorno de ejecución en ninguno de los cuatro niveles y no está en modo sin conexión, llama a `install::install(dir, "latest")`, instalando en `~/.dsh/dsh-tauri/runtime`:

```
runtime::ensure          定位失败 → 非离线 → install_fresh
  install::npm_available 先跑 `npm --version`，缺 npm 给可读错误
  swap::Staging::new     建 <runtime>.new-<pid> 暂存目录（先清掉旧的临时树）
  exec::run              npm install --prefix <staging> --no-audit --no-fund
                          --loglevel=error @deepseek-ai/dsh@latest（600 秒超时）
  has_package            校验 node_modules/@deepseek-ai/dsh/lib/bin.js 存在
  Staging::commit        rename 换上去，旧的改名 .old-<pid>
```

- Durante la instalación se establece `progress::set(Stage::Installing, …)` y splash sondea `runtime_status` para mostrar la fase y el tiempo transcurrido;
- `exec.rs` ejecuta npm en su propia sesión y recoge todo el grupo de procesos al agotar el tiempo de espera o al salir, sin dejar procesos nietos de `postinstall`;
- Cualquier fallo devuelve un error legible más el comando manual, pasando por `progress::fail` → página de error.

### Implementación de la actualización automática

Tras arrancar el servicio, `update::spawn_once(dir, source)` inicia un hilo `dsh-update` (se ejecuta una sola vez por proceso):

1. `version_at(dir)` lee la versión del `node_modules/@deepseek-ai/dsh/package.json` local;
2. `npm view @deepseek-ai/dsh version` obtiene la versión de upstream (tiempo de espera de 90 segundos, respetando la configuración de espejo npm del usuario);
3. `version::is_newer` compara (comparación semántica pura en Rust, incluidas las reglas de prerelease; véanse las pruebas unitarias de `version.rs`);
4. Si hay actualización, `progress::set(Stage::Updating, …)` → `install::install(dir, "latest")` sigue el mismo flujo de staging + reemplazo atómico → `notify-send -a DSH "DSH 已更新" "已更新到 x.y.z，重启生效"`.

Restricciones: no matar el servicio en ejecución (la versión nueva se carga solo en el próximo inicio), en caso de fallo imprimir solo `[runtime] 更新检查失败（下次启动再试）` y reintentar en el próximo inicio, y cuando `Source::updatable()` es falso, escribir solo una línea de registro.

### Reemplazo atómico y limpieza

- El directorio de staging `<runtime>.new-<pid>` y el árbol antiguo `<runtime>.old-<pid>` son **directorios hermanos** del entorno de ejecución, lo que garantiza que `rename` se realice en el mismo sistema de archivos y de forma atómica;
- `commit` primero renombra el directorio antiguo a `.old-<pid>` y luego renombra el staging a su lugar; si el segundo paso falla, el directorio antiguo se renombra de vuelta (el mensaje de error indica «旧运行时已恢复»);
- `Staging::drop` elimina el árbol de staging cuando no hubo commit: una instalación interrumpida no deja un producto a medias;
- El árbol antiguo se **conserva para el servicio aún en ejecución** (sus módulos se cargaron desde el árbol antiguo), por lo que tras una actualización conviven dos copias en disco (unos 1.2G); `swap::clean_stale_runtime()` se llama en el `.setup` de `main.rs`, después de ganar el bloqueo de instancia única, y limpia todos los hermanos `<name>.new-*` / `<name>.old-*`, registrando solo los fallos.

## 5. Configuración y variables de entorno

`cargo run` inicia el servicio por sí mismo y lo abre en una ventana. Opciones habituales de sobrescritura:

| Variable | Función |
| --- | --- |
| `DSH_TAURI_RUNTIME` | directorio del entorno de ejecución (que contiene `node_modules`) |
| `DSH_TAURI_NODE` | ejecutable de node |
| `DSH_TAURI_HOST` | host al que se enlaza el servicio, solo loopback (por defecto `127.0.0.1`; se permite `localhost` / `::1`) |
| `DSH_TAURI_NPM` | ruta al ejecutable de npm (por defecto el npm del PATH; lo usan la instalación/actualización automática y la detección de versión) |
| `DSH_TAURI_PROFILE` | qué perfil arrancar (por defecto `web`) |
| `DSH_HOME` | directorio de datos (por defecto `~/.dsh`) |
| `DSH_TAURI_URL` | para desarrollo: abrir directamente un servicio loopback ya en ejecución (solo http(s) + 127.0.0.1/localhost/[::1]), sin lanzar uno |

### Interruptores de actualización automática (variables de entorno)

| Variable | Función |
| --- | --- |
| `DSH_TAURI_AUTO_UPDATE=0` | desactiva la comprobación de versión; la instalación automática del primer inicio sigue ejecutándose |
| `DSH_TAURI_OFFLINE=1` | desactiva toda actividad de red (instalación + comprobación); se puede arrancar sin conexión con un entorno de ejecución ya existente |

### Los dos interruptores (valores y efectos)

| Variable | Valores | Efecto |
| --- | --- | --- |
| `DSH_TAURI_OFFLINE` | `1/true/yes/on` | desactiva la instalación y la comprobación (toda actividad de red); página de error si no hay entorno de ejecución |
| `DSH_TAURI_AUTO_UPDATE` | `0/false/no/off` | desactiva solo la comprobación de versión; la instalación automática del primer inicio sigue ejecutándose |

La implementación está en `runtime.rs::offline()` / `auto_update_enabled()`; `auto_update_enabled` cortocircuita `OFFLINE` primero, por lo que en modo sin conexión ambos están desactivados.

## 6. Notas clave de implementación

1. **La URL del servicio se obtiene únicamente de stdout**: `dsh web` imprime `dsh web: http://127.0.0.1:<port>/?token=…`; no adivines el puerto por tu cuenta: `--port 0` deja que el sistema lo asigne.
2. **La ventana debe crearse directamente en la URL una vez obtenida**. Crear primero una ventana splash y luego hacer `navigate` hace que WebKit pierda el `Set-Cookie` de la redirección de autenticación (síntoma: la página muestra solo `dsh web authentication required`). Por ello se usa una ventana splash con una etiqueta independiente y, cuando se obtiene la URL, se crea una ventana `main` nueva y se destruye la splash.
3. **El script de inyección debe ejecutarse antes de los scripts de la página**: `WebviewWindowBuilder::initialization_script`. Depende de `window.__TAURI__.core.invoke`, proporcionado por `withGlobalTauri` (véase `tauri.conf.json`).
4. **El token de la cadena de consulta es un punto de entrada de autenticación de un solo uso**: `GET` devuelve 303 y emite una cookie `dsh-auth-<authority>` (`HttpOnly; SameSite=Strict`). El servidor genera el nombre de la cookie según la autoridad, por lo que cambia cuando cambia el puerto.
5. **La salida se realiza mediante SIGTERM**: `dsh::shutdown()` ejecuta primero `kill -TERM` y solo aplica SIGKILL si después de 5 segundos no ha salido, para evitar dejar residuos WAL en la base de datos de sesiones.
6. **La salida desde la bandeja** debe llamar a `dsh::shutdown()` antes de `app.exit(0)`; de lo contrario quedan procesos node huérfanos.

## 7. Comandos de verificación

```bash
node --check tools/probe-web.mjs
npm test                                     # 拉起 dsh web，验证 303 + cookie + 客户端 HTML
cd src-tauri && cargo test && cargo build    # Rust 单测 + 构建
```

`npm test` requiere que el entorno de ejecución esté instalado previamente:

```bash
npm install --prefix runtime @deepseek-ai/dsh
```

### Depuración de la instalación y las actualizaciones automáticas

**npm falso** (sin red, sin descargar 1.2G; verifica toda la cadena de instalación/actualización): `install::npm()` reconoce `DSH_TAURI_NPM`; basta con escribir un script ejecutable que suplante a npm.

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

- **Instalación en el primer inicio**: aparta `~/.dsh/dsh-tauri/runtime` (`mv` a `/tmp`), ejecuta `DSH_TAURI_NPM=/tmp/fake-npm cargo run`; splash debe entrar en la fase «正在安装» y, al terminar, debe existir `~/.dsh/dsh-tauri/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js`; el paquete falso no tiene un servicio real, así que la ventana acabará deteniéndose en un fallo de arranque: basta para verificar la cadena de instalación.
- **Ruta de actualización**: tras la instalación, define `FAKE_LOCAL=1.0.0` y `FAKE_UPSTREAM=2.0.0` y vuelve a arrancar; el registro debe mostrar `发现新版 2.0.0（本地 1.0.0）`, `已原子替换`, `旧运行时保留至下次启动清理`, y aparece un hermano `<runtime>.old-<pid>`; al arrancar una vez más debe imprimir `清理上次运行留下的 …`.
- **Interruptores**: `DSH_TAURI_OFFLINE=1` y sin entorno de ejecución → el cuerpo de la página de error incluye el comando `npm install --prefix`; `DSH_TAURI_AUTO_UPDATE=0` → el registro muestra `自动更新已关闭`, pero la primera instalación se sigue ejecutando.
- **Fallo de instalación**: haz que la rama `install` del npm falso ejecute `exit 1` → la página de error incluye la salida de npm y el comando manual, y el entorno de ejecución original no se ve afectado (`<runtime>.new-<pid>` fue eliminado por `Drop`).
- **Limpieza final**: `pkill -x dsh-tauri`; confirma que no quedan restos `.new-*` / `.old-*` bajo `~/.dsh/dsh-tauri/` y recuerda eliminar/devolver el npm falso y el entorno de ejecución apartado.

### Comprobación rápida (manual)

```bash
cd src-tauri && cargo run          # 应看到 splash，随后出现完整 UI
curl -s -o /dev/null -w '%{http_code}\n' "$(grep -ao 'http://127.0.0.1:[0-9]*/?token=[A-Za-z0-9_-]*' /tmp/xxx.log | head -1)"
```

La ventana debe mostrar: lista de sesiones, espacios de trabajo, selector de modelo y flujos de sesión en curso.

## 8. Verificado (Linux + WebKitGTK 2.52)

- El envoltorio lanza `dsh web` y analiza `http://127.0.0.1:<port>/?token=…` desde stdout; la primera pantalla aparece en 3–30 segundos (según la caché);
- Cadena de autenticación: URL con token → 303 + cookie `dsh-auth-*` → `GET /` devuelve el HTML del cliente (`tools/probe-web.mjs`);
- La ventana contiene la Web UI completa: lista de sesiones, espacios de trabajo, selección de modelo, páginas de plugins y flujos de sesión en curso están presentes;
- La ventana cambia de nombre con `document.title`; al cerrar se sigue la preferencia de «cerrar a la bandeja» o «salir» (la preferencia es el campo `closeToTray` en `~/.dsh/desktop-preferences.json`: `true` = cerrar a la bandeja, `false` = salir, por defecto `true`); el tamaño se restaura al reiniciar;
- Un segundo inicio enfoca la ventana existente y sale, sin arrancar un segundo servicio;
- El servicio se reinicia automáticamente tras ser matado (máximo 3 veces); si sigue fallando, se muestra una página de error legible;
- En el primer inicio sin entorno de ejecución, `npm install` se ejecuta automáticamente en `~/.dsh/dsh-tauri/runtime` y splash muestra la fase actual y el tiempo transcurrido;
- El servicio arranca automáticamente tras la instalación; si existe una versión de upstream más nueva, se instala en segundo plano y `notify-send` muestra «重启生效», sin afectar al servicio en ejecución;
- Con `DSH_TAURI_OFFLINE=1` y sin entorno de ejecución local, se muestra una página de error con el comando manual en lugar de una ventana en blanco;
- Las actualizaciones pasan por un directorio de staging `<runtime>.new-<pid>` + reemplazo atómico, de modo que una instalación interrumpida no daña el entorno de ejecución ya instalado.

## 9. Limitaciones conocidas / límites

| Elemento | Estado actual |
| --- | --- |
| Vista incrustada de la página de la plataforma (uso / recarga) | Se abre en el navegador; no se ha implementado incrustación nativa |
| El entorno de ejecución antiguo ocupa disco tras la actualización | El árbol antiguo se renombra `<runtime>.old-<pid>` y se conserva para el servicio aún en ejecución; **se limpia solo en el próximo inicio**; durante ese tiempo ocupa aproximadamente una copia adicional del entorno de ejecución (unos 1.2G) |
| Residuos por SIGKILL | Cuando el envoltorio recibe `SIGKILL` no tiene tiempo de recoger los procesos hijos, por lo que los procesos nietos de `postinstall` de npm pueden permanecer brevemente; la salida normal (SIGTERM) no se ve afectada |
| Windows / macOS | El código está escrito contra comandos nativos de Linux (zenity/wl-clipboard/notify-send); otras plataformas necesitan sustituirlos |
| Posición de la ventana en Wayland | La decide el compositor; `position()` solo afecta a compositores de estilo X11 |
| Espera en el primer inicio | Se pueden ver la fase y el tiempo transcurrido, pero no hay salida de npm línea por línea |

## 10. Notas

Este proyecto no está oficialmente relacionado con DeepSeek y no contiene binarios oficiales; el entorno de ejecución lo instala el usuario con los comandos anteriores. El código se publica bajo licencia MIT, véase [LICENSE](../LICENSE).
