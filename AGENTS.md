# AGENTS.md

Guía para agentes de código (Claude Code, Codex, etc.) que trabajen en `mer`.

## Qué es

`mer` es un emulador de terminal 100 % Rust del mantenedor, hecho para su propia máquina
(Fedora, KDE Plasma en Wayland). Prioridades, en orden: rapidez y ligereza,
espacios que duermen (no consumen), detección de agentes (Claude Code, Codex), interfaz simple,
panel de ajustes y estructura mantenible a largo plazo. La máquina del mantenedor es la
referencia: no añadas compatibilidad con otras plataformas sin que se pida. Las releases públicas
(.rpm/.deb para x86-64 genérico) existen desde el ADR 0006.

Remoto: `git@github.com:rodasOS/mer.git`. Licencia MIT.

## Stack

- Rust 1.96, edición 2024, fijado en `rust-toolchain.toml`. `unsafe` denegado.
- UI: GPUI (Zed), fijado a un commit concreto en `Cargo.toml`; no lo actualices sin pedirlo.
  `vendor/gpui_wgpu` es una copia local de esa crate con un solo cambio (Vulkan primero, ADR 0007; ver su
  README): al subir el fijado de GPUI hay que rehacerla desde la nueva revisión.
- Motor de terminal: `alacritty_terminal` 0.26.
- Persistencia: SQLite (`rusqlite` + `rusqlite_migration`) y `zstd` para el historial.
- IPC: socket Unix, JSON de una línea con `{"v":1,…}`.
- Config: `toml_edit` + `notify`. Notificaciones: `notify-rust`.
- Compilación con `RUST_FONTCONFIG_DLOPEN=1` y `target-cpu=native` (ver `.cargo/config.toml`).
  No hay sudo ni paquetes `-dev`.

## Estructura

Workspace de 8 crates + `xtask`:

| Crate | Responsabilidad |
|---|---|
| `mer-core` | Modelo puro: espacios, pestañas, paneles, `Accent`, dormir/borrar. Sin ventana. |
| `mer-term` | Envoltorio del motor de terminal y PTY. |
| `mer-agents` | Detección de agentes y hooks. |
| `mer-store` | SQLite, migraciones, estado de UI (`UiState`), escritor en segundo plano. |
| `mer-config` | `config.toml`, esquema y valores por defecto. |
| `mer-ipc` | Protocolo (`CtlCommand`), cliente y servidor del socket. |
| `mer-ui` | GPUI: `WorkspaceView`, `TerminalView`, diálogos, menús, ajustes. |
| `mer` | Binario y CLI (`mer`, `mer ctl …`, `mer quit`). |
| `xtask` | `cargo xtask ci | pgo | install | uninstall`. |

Regla: la lógica va en los crates sin ventana (se prueba sin GPU); `mer-ui` solo dibuja y
conecta. Documentación: `README.md`, `docs/` (arquitectura, atajos, configuración, agentes,
performance) y `docs/adr/` (decisiones 0001–0009).

## Arquitectura que no se debe romper (ADR 0005)

- `WorkspaceView` se crea **una vez, sin ventana**, y vive en el global `MainView`. Las ventanas
  se acoplan (`attach`) y desacoplan (`window_closed`). Esto permite la opción
  `spaces.keep_running_on_close` (apagada por defecto): sin ventana los terminales siguen vivos.
- Ninguna función del view recibe `Window`. El foco se pide con `want_focus: Option<Focus>` y se
  aplica en `render` con `window.defer`.
- Las tareas de fondo (`watch`, auto-sleep, IPC) usan `cx.spawn`, **no** `spawn_in`.
  `TerminalView::new(launch, cx)` y sus suscripciones tampoco dependen de la ventana.
- Cierre: `QuitMode::Explicit` + `on_window_closed` decide salir o seguir. `mer -e` nunca sigue.
- IPC: lo que muestra UI (`Open`, `Show`, `ShowWindow`, `SpaceNew`, `SpaceWake`) abre la ventana
  primero (`needs_window_for`); hooks y consultas se atienden sin ventana.
- Todos los diálogos usan `workspace_view/dialog.rs` (`render_prompt`); los menús contextuales
  están en `workspace_view/context_menu.rs`. No dibujes modales sueltos.
- Estilo de la interfaz: usa los tokens de `theme.rs` (`Palette::hover()`, `hairline()`,
  `overlay_shadow`) y los radios `rounded_lg` (filas, botones), `rounded_xl` (tarjetas, menús,
  diálogos) y `rounded_full` (píldoras). Las sombras solo van en menús, diálogos y ajustes.
  Los chips de tecla (`tooltip::kbd`) y los tooltips (`tooltip::tip`) son componentes comunes.
- Tamaño de la interfaz (`appearance.ui_scale`): las medidas en píxeles de la interfaz pasan
  por `self.ui_px(v)` (o `Ui::px` en Ajustes); las medidas en `rem` (`p_2`, `text_xs`,
  `rounded_*`) escalan solas con `window.set_rem_size`. La terminal NO escala con esto: tiene su
  propio `font_size` y su zoom.
- Estados de agentes: la lógica pura vive en `mer-agents` (`AgentStatus::after`, `after_interrupt`,
  `after_submit`, `parse_claude_payload`). No metas lógica de estados en `mer-ui`.
- Nombre de pestañas: `Tab::display_title` (en `mer-core`) elige nombre del usuario → título/comando/etiqueta
  del `Program` en primer plano **solo si `Program::visible`** (lleva ~800 ms; el temporizador de un disparo está
  en `set_program`) → nombre de la carpeta (`Pane::folder_name`) → título del shell → `shell`. El prompt (`mer:ctx:`)
  borra el programa al instante si el shell está libre (`on_prompt_report`). La ventana usa `display_title()`, nunca
  su propia regla. `Pane.program` es solo en vivo (`serde(skip)`).
  Los títulos que empiezan con `mer:cmd:` son del canal privado de la integración de shell y nunca
  se muestran. Los tooltips (`tooltip::tip`) no parten líneas: GPUI los maqueta a ancho mínimo.
- Pintado de la terminal (`terminal_view.rs`): copiar celdas bajo el candado (`copy_cells`), soltarlo
  y armar tramos (`build_runs`) con buffers reutilizados (`PaintScratch`). No vuelvas a estilar con el
  candado tomado. La pendiente de optimizaciones con costo está en `TODO.md`.
- Barra de scroll (`mer-term/src/scrollbar.rs`, aritmética pura con tests): `TerminalView` lee `scroll_state()`
  (un solo candado) y dibuja pulgar y botón «ir al final»; se oculta con un temporizador de un disparo
  (`flash_scrollbar`), nunca con sondeos. No se muestra en pantalla alterna.
- Chips de rama git y node (`workspace_view/context.rs`, ADR 0008): el estado es `PaneContext` (`mer-core`),
  alimentado por el título privado `mer:ctx:` del prompt de zsh (`on_title` lo traga: nunca es un título).
  La rama es una lectura de `.git/HEAD` en el executor de fondo; las listas se leen solo al abrir el selector.
  **Nunca se escribe en la PTY si el shell no está en el prompt o hay un programa en primer plano**
  (`PaneContext::accepts_commands` + `mer_agents::foreground`, comprobado al abrir y al elegir). No leas
  `/proc` durante el render: la atenuación de los chips usa `pane.program` y `at_prompt`.
  La versión que pide el proyecto (`.nvmrc`, `PaneContext::required_node`/`node_fit`) se lee **perezosa**:
  solo si cambia `(cwd, rama)`, en el executor de fondo y al abrir el selector; nunca se ejecuta `nvm` ni `node`.
- Ajustes (`workspace_view/settings_panel.rs`): un `Section` por panel y un `render_*`; los paneles grandes viven en
  `settings_panel/` (Performance). El panel solo agrupa: las claves de `config.toml` no cambian de tabla.
  Lo opcional que cuesta algo (scrollback, barra, chips, hint de node) va en **Performance**, con su costo en la
  descripción; una función opcional nueva se añade ahí y a `set_optional`.
- Búsqueda (`mer-term/src/search.rs`, lógica pura con tests; barra en `terminal_view.rs`): el resaltado
  solo mira las líneas visibles y solo corre mientras la barra está abierta. `TerminalView::focus_handle()`
  devuelve el cuadro de búsqueda mientras está abierto, para que `focus_active` no le robe el foco.
- Lado de la barra lateral: nada compara con izquierda/derecha suelto; todo pasa por `SidebarSide`
  (`mer-config`): orden, ancho a partir de `x`, flechas.
- Cambiar el programa de un panel pasa SIEMPRE por `WorkspaceView::set_program` (mide cuánto duró para el
  aviso de comando largo). Mover pestañas/espacios: `Workspace::move_tab`, `move_space`, `move_tab_to_space`
  (un espacio nunca queda sin pestañas); la interfaz del arrastre está en `workspace_view/drag.rs`.
- Terminales sueltas (ADR 0009): `Space` tiene un `SpaceKind` (`Space` | `Terminal`). **Una terminal es un espacio con
  exactamente una pestaña** (puede tener paneles): `add_tab` dentro de una terminal crea otra terminal, nunca se mueve una
  pestaña a una terminal, `adopt_terminal`/`detach_tab`/`promote_to_space` son las únicas conversiones, y `from_data`
  rechaza una terminal con ≠1 pestañas. El texto de una fila sale de `Space::display_name()` (no de `space.name`, vacío en
  una terminal) y el del cuadro de renombrar de `editable_name()`. El sidebar vive en `workspace_view/sidebar.rs`: colapsado
  **no se dibuja** (`render_sidebar` devuelve `None`; no hay riel). `render_sidebar_toggle` está en la cabecera del sidebar
  y, oculto, en la barra de pestañas; una terminal suelta no tiene barra (`render_tab_bar` devuelve `None`) salvo una
  franja de 28 px con el botón mientras el sidebar está oculto. Un campo nuevo en `spaces` = migración nueva en `db.rs` y `#[serde(default)]`.
- Dormir: una pestaña dormida no carga procesos; la pestaña visible nunca carga dormida.
- Cambios de esquema de SQLite = migración nueva (`db.rs`); los datos nuevos llevan
  `#[serde(default)]` para no romper bases viejas.

## Comandos

```sh
cargo run                      # compila y abre mer (usa TU estado: ver "Probar")
cargo xtask ci                 # fmt --check + clippy -D warnings + test (hazlo antes de commitear)
cargo xtask pgo                # perfil PGO (~25 min)
cargo xtask install --pgo      # instala en ~/.local/bin/mer
cargo xtask uninstall
```

Clippy va en `pedantic` con `-D warnings`. Un `#[allow]` lleva comentario con el porqué.

**Instalación local = `target-cpu=native`.** `.cargo/config.toml` lo fija y `cargo xtask install`
nunca debe poner un objetivo genérico. El nivel genérico (`x86-64-v2`) y la feature `x11` solo
se usan en `.github/workflows/release.yml` (tag `v*` o ejecución manual), que genera el `.deb` y
el `.rpm`; `release-windows.yml` es manual y falla hasta portar el código (ver `TODO.md`).
Decisión en `docs/adr/0006-public-releases.md`.

**Dependencias: siempre actuales y sin vulnerabilidades reportadas.** Antes de publicar o de subir
una dependencia, ejecuta `cargo deny check` y `cargo audit` (ambos deben salir sin vulnerabilidades).
Las acciones de GitHub se fijan por SHA completo con su versión en un comentario y deben usar
Node 24; Dependabot (`.github/dependabot.yml`) propone las actualizaciones. GPUI sigue fijado
(ADR 0001): los avisos de "sin mantenimiento" que vienen de él están en `TODO.md`.

## Probar sin tocar el estado del mantenedor

Estado aislado y **ruta corta** (el socket Unix limita la ruta a ~100 caracteres):

```sh
I=/tmp/mer-s; mkdir -p $I/{state,config,run}
XDG_STATE_HOME=$I/state XDG_CONFIG_HOME=$I/config XDG_RUNTIME_DIR=$I/run \
WAYLAND_DISPLAY=/run/user/$(id -u)/wayland-1 cargo run
```

- **El shell de Claude Code hereda `MER_SOCKET` y `MER_PANE_ID` de la instancia real del mantenedor.**
  Ejecuta el cliente de prueba con `env -u MER_SOCKET -u MER_PANE_ID …`; si no, `mer`/`mer ctl`
  le hablan a su instancia.
- Una ventana viva y tapada no se repinta con `notify`. Para capturar un estado nuevo cierra la
  ventana (`mer ctl close-window`) y ábrela con `mer ctl show <qué>` o `mer ctl window`.
- Para simular agentes: el id de cada panel está en `MER_PANE_ID` de `/proc/<pid del shell>/environ`
  y se manda con `mer hook --agent claude` (JSON por stdin) o `--agent codex '<json>'`.
- `pw-play` dentro de un `XDG_RUNTIME_DIR` aislado falla: añade `PIPEWIRE_RUNTIME_DIR=/run/user/$(id -u)`.
- Para ver la interfaz sin teclado ni mouse: `mer ctl show <palette|rename-space|rename-tab|
  confirm-delete|confirm-sleep|space-menu|tab-menu|pane-menu|sidebar-menu|settings:<sección>>`,
  `mer ctl show <branch-picker|node-picker>`, `mer ctl show settings:<appearance|terminal|spaces|agents|performance|shortcuts>`, `mer ctl zoom in|out|reset`, `mer ctl ui-scale in|out|reset`, `mer ctl sidebar collapse|expand`, `mer ctl terminal new [--cwd DIR]`,
  `mer ctl sidebar-width <px>`, `mer ctl space list`.
- Capturas: `spectacle -a -b -n -o <archivo>` con la ventana **activa**; comprueba el tamaño
  (≈1130x798) antes de mirarla, porque puede capturar la ventana equivocada.
- Abrir una ventana de prueba le **roba el foco y las pulsaciones** al mantenedor. Avísale antes, y
  para capturas usa un shell sin eco (`stty -echo; exec sleep …`). No captures su sesión de
  trabajo ni la pantalla completa.
- Matar procesos de prueba **por pid** (o `pkill -x <nombre>` exacto). Nunca `pkill -f`: puede
  matar tu propio shell. Evita `pkill -x sleep`, que alcanza los `sleep` del mantenedor.
- La regla de posición de ventana toca KWin (`kwriteconfig6`, `positionrule`); no la cambies
  en pruebas.

## Convenciones

- **Idioma:** código, identificadores, comentarios, commits y ADR en inglés. README y `docs/`
  (documentación de usuario) en español.
- **Commits:** Conventional Commits en imperativo, < 72 caracteres (`feat(ui): add terminal text
  zoom`). Un commit por cambio lógico. Se usa el skill `commit:commit` indicando
  "solo commit, NO push". **Nunca hagas push.**
- **Pruebas:** cada cambio de comportamiento lleva su test. Verifica con evidencia (tests,
  capturas, `ps`, `mer ctl`) y di con honestidad lo que no pudiste probar (clics reales,
  Ctrl+rueda, atajos de teclado, reapertura desde el lanzador).
- **Atajos:** viven en `actions.rs` (`[keys]` en la config); admiten varias teclas con `|`.
  Hay tests de que parsean y no se repiten. Documenta los nuevos en `docs/atajos.md`.
- **Documentación:** si cambias comportamiento visible, actualiza `docs/` y `CHANGELOG.md`; si
  cambias algo entre crates o revocas una decisión, escribe un ADR nuevo.
- **Rendimiento:** referencia medida (PGO) en `docs/performance.md`: primer frame ~312–344 ms,
  CPU en reposo 0, RSS ~146–158 MiB (155 MiB con ventana tras la ronda de interfaz moderna). No metas trabajo en el hilo de render ni sondeos
  periódicos; usa eventos.
- **Privacidad:** no copies credenciales ni notas personales del mantenedor a ningún archivo del
  repo; no incluyas rutas de trabajo ni datos personales en capturas o docs.

## Límites conocidos

- Con `keep_running_on_close`, si `mer` se cae las terminales mueren con él (no hay demonio
  aparte, ADR 0003/0005). No hay icono de bandeja, por decisión del mantenedor.
- Wayland no deja subir al frente una ventana ya abierta (`activate` no implementado).
- `release-windows.yml` nunca se ha ejecutado y fallará hasta portar el código (ver `TODO.md`). Los
  demás workflows (ci, audit, release) ya corrieron en GitHub. No hay confirmación al cerrar una
  pestaña con un programa corriendo.
