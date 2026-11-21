# Changelog

Formato basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/). El proyecto
usa versionado semántico mientras no llegue a 1.0, las versiones `0.x` pueden cambiar sin aviso.

## [Sin publicar]

### Añadido
- **Releases en GitHub**: un workflow crea el `.rpm` y el `.deb` al subir una etiqueta `v*`
  (o a mano desde Actions). Se compilan para `x86-64-v2` y sin PGO. Hay también un workflow
  manual para Windows que fallará hasta portar el código.
- Feature `x11` (apagada por defecto) para abrir ventanas también en X11.
- README: cómo compilar para tu CPU, y metadatos de paquetes en `crates/mer/Cargo.toml`.
- README reescrito: instalación con paquete o desde el código, solución de problemas, seguridad y
  cómo publicar una versión. Capturas nuevas (barra a la derecha, menú con *Move up/down*,
  ajustes con el tamaño de la interfaz y el lado de la barra).
- Workflow `audit.yml`: `cargo deny check` en cada push y cada semana (vulnerabilidades,
  licencias y fuentes). `deny.toml` ya se ejecutó y pasa; Dependabot propone actualizaciones.

### Seguridad
- La carpeta de estado, la base de datos y el historial se crean privados (`0700`/`0600`); lo
  existente se corrige al arrancar. El historial tiene un límite de tamaño al descomprimirse.
- Pegar ya no puede ejecutar texto ajeno: se quitan los caracteres de control y el cierre del
  "pegado entre corchetes" incrustado.
- La instalación de hooks conserva permisos y enlaces simbólicos, no pisa la primera copia de
  seguridad y escapa la ruta del ejecutable; la carpeta del socket se corrige a `0700`.
- Workflows: la etiqueta ya no se interpola en un script y las releases no usan caché.
- CI: compila con `x86-64-v2` y una caché nueva para no depender de la CPU del runner; `audit.yml`
  ya no corre en cada pull request, solo si cambian las dependencias.
- Dependencias actualizadas (`notify` 9 y el resto de versiones compatibles): `cargo audit` no
  reporta vulnerabilidades. Las acciones de GitHub usan Node 24 y están fijadas por SHA.

### Corregido
- Los **tooltips** ya no parten el texto en dos líneas que se superponían: las líneas largas se
  acortan por el medio y se admiten varias líneas de detalle.

### Rendimiento
- El terminal se bloquea menos tiempo al pintar (copia las celdas y suelta el candado), reutiliza
  buffers, usa una tabla de 256 colores precalculada y arma una sola fuente por tramo de texto.
- Arranque: la base de datos se abre una vez, no tres.
- El registro de agentes se construye al cambiar la configuración, no en cada ráfaga de salida.
- Al dormir o cerrar, el historial de varias terminales se captura en paralelo.

### Añadido
- **Shift+Enter** inserta un salto de línea en Claude Code y otros agentes (envía `ESC CR`, igual
  que Alt+Enter) en vez de enviar el mensaje.
- **Buscar en el historial** (`Ctrl+Shift+F`, o «Search in terminal» en la paleta): una barra
  sobre la terminal enfocada que resalta los resultados en pantalla y en el historial, cuenta
  cuántos hay y salta entre ellos (`Enter` o ↑ va al anterior, `Mayús+Enter` o ↓ al siguiente,
  `Esc` cierra). El texto se busca tal cual, sin importar mayúsculas salvo que escribas alguna.
  `mer ctl search <texto>`.
- **Volumen de los sonidos** (`agents.sound_volume`, de 0 a 100, 50 por defecto; antes sonaban
  al máximo). Ajustes → Agents.
- **Aviso de comando largo** (`agents.long_command_seconds`, 30 por defecto, 0 lo apaga): una
  notificación y un sonido cuando un comando largo termina y no lo estabas mirando. Ajustes →
  Agents.
- **La barra lateral va a la derecha por defecto** (`appearance.sidebar_side = "left" | "right"`,
  también en Ajustes → Appearance). El asa de arrastre, el riel y las flechas de plegar se
  acomodan al lado elegido.
- **Arrastrar y soltar**: las pestañas se reordenan arrastrándolas, los espacios también en la
  barra lateral, y una pestaña se mueve a otro espacio soltándola sobre él (con sus terminales
  vivos; un espacio siempre conserva al menos una pestaña). Alternativas sin mouse: entradas en
  los menús de clic derecho y en la paleta, `move_tab_left/right` y `move_space_up/down` en
  `[keys]`, y `mer ctl tab move|to-space`, `mer ctl space move`.
- **Las pestañas dicen qué corre**: `npm run dev`, `vim notas.md`, `claude`… con la carpeta
  atenuada al lado, en vez del título del shell. Un programa que pone su propio título
  (Claude Code, vim) lo conserva, y tu nombre manual siempre gana.
- **Integración de shell (zsh)** opcional: `mer shell-init zsh` y Ajustes → Agents → *Shell
  integration*. Con ella la pestaña muestra el comando exacto (`ionic serve`) aunque el programa
  reescriba su nombre de proceso. Solo actúa dentro de `mer`.
- **Sonidos** cuando un agente te necesita o termina (`agents.sound`, `sound_waiting` y
  `sound_done`; Ajustes → Agents con botón *Test*). Respetan «No molestar» de KDE.
- **Barra lateral redimensionable**: se arrastra su borde derecho (160 a 480 px; si la cierras
  de un arrastre pasa al riel) y un doble clic restablece el ancho. Se recuerda entre sesiones
  y hay `mer ctl sidebar-width <px>`.
- **Tamaño de la interfaz** (`appearance.ui_scale`, 0.8 a 1.6) aparte del zoom de la terminal:
  `Ctrl+Alt+=`, `Ctrl+Alt+-`, `Ctrl+Alt+0`, Ajustes → Appearance y `mer ctl ui-scale`.
- **Tooltips** con el nombre completo (y la carpeta) al pasar el mouse por pestañas y espacios.
- **Seguir corriendo con la ventana cerrada** (opción `spaces.keep_running_on_close`, apagada
  por defecto). `mer quit`, `Ctrl+Shift+Q`, y `mer` sin argumentos muestra la ventana de la
  instancia que corre. `mer ctl window`, `close-window` y `quit`.
- **Zoom del texto** de la terminal: `Ctrl+=`, `Ctrl++`, `Ctrl+-`, `Ctrl+0`, `Ctrl` + rueda,
  paleta y `mer ctl zoom`. Una acción puede tener varias teclas (`"ctrl-= | ctrl-+"`).
- **Menús con clic derecho** sobre espacios, pestañas, terminales y la barra lateral.
- **Colores** para espacios y pestañas (diez tonos); los espacios nuevos reciben uno distinto.
- **Barra lateral plegable** en un riel, con botón de ajustes siempre visible.
- **Dormir pestañas sueltas** y **borrar espacios** con confirmación.
- Se recuerdan el **tamaño de la ventana** y el estado de la barra lateral; botón en Ajustes
  para pedirle a KDE que recuerde también la posición.
- Documentación: README, guías en `docs/`, ADR 0005, `LICENSE`, `CONTRIBUTING.md`,
  `SECURITY.md`, integración continua y `cargo xtask ci`.
- `mer ctl show …`, `space color|delete`, `tab sleep|color`, `sidebar` para automatizar y
  probar sin mouse.

### Cambiado
- La sección **AGENTS** va ordenada por urgencia: el agente que espera es una tarjeta resaltada
  con «Needs your attention», cada estado tiene su píldora, la cabecera cuenta cuántos esperan
  y, en el riel, un agente que espera se ve como un anillo.
- **Interfaz más moderna y redondeada**: los terminales son tarjetas con un hueco entre ellas,
  filas y pestañas en forma de píldora, chips para las teclas, menús y diálogos con sombra,
  Ajustes agrupados en tarjetas con interruptores. Las sombras solo existen mientras hay un
  menú o diálogo abierto.
- Las pestañas toman el ancho que necesitan (hasta 360 px) y solo se acortan cuando no caben.
- Los estados de los agentes son más fieles: tras aprobar un permiso vuelve a «trabajando»
  (hook `PostToolUse`, vuelve a pulsar *Preview install…* para añadirlo), los avisos de
  inactividad y el fin de un subagente se ignoran, Esc/Ctrl+C sacan al agente de «trabajando»,
  y un agente sin hooks se muestra como «detected».
- Los diálogos (paleta, renombrar, confirmaciones) se rediseñaron: título, texto que se
  ajusta, campo de texto visible y botones con el mouse.
- La base de datos pasa a la versión 2 (colores y pestañas dormidas). Los espacios existentes
  reciben un color al migrar.
- `mer` sin argumentos con una instancia corriendo ya no abre una pestaña nueva: muestra la
  ventana (usa `mer ctl open <carpeta>` para la pestaña).

### Corregido
- Abrir los ajustes o un diálogo deja de apilarse sobre otros diálogos o menús.
- La lista de shells de los ajustes ya no repite rutas ni ofrece `tmux`.

## [0.1.0]

Primera versión de uso.

- Terminal con GPUI y el motor de Alacritty: teclado, selección y portapapeles de Wayland,
  mouse en programas de pantalla completa, historial y redimensionado.
- Espacios, pestañas y paneles divididos; paleta de comandos.
- Espacios que duermen y despiertan, con carpeta, disposición e historial restaurados
  (SQLite + zstd); sueño automático opcional.
- Detección de Claude Code, Codex CLI, Gemini CLI y opencode, por proceso y por hooks;
  notificaciones de escritorio; reanudar la sesión de Claude Code (opcional).
- Panel de ajustes, temas en TOML, atajos configurables y recarga en caliente.
- `mer ctl`, `mer hook` y `mer -e`.
- `cargo xtask install`, `pgo` y `bench`.
