# Arquitectura

`mer` es un solo proceso. Un workspace de Cargo con un crate por responsabilidad; solo
`mer-ui` y el binario dependen de GPUI.

```text
mer ──► mer-ui ──► mer-term ──► alacritty_terminal (PTY, parser, cuadrícula)
          │   ╲
          │    ╲──► mer-agents   (procesos, hooks)
          │     ╲─► mer-store    (SQLite, historial .zst)
          │      ╲► mer-config   (config.toml, temas, recarga)
          └─────────► mer-ipc    (socket de control)
        todos ──► mer-core       (modelo puro, sin UI ni IO)
```

## El modelo (`mer-core`)

`Workspace` contiene `Space`s; cada uno tiene `Tab`s; cada pestaña tiene un árbol de divisiones
(`Layout`) cuyas hojas son `Pane`s. Un panel guarda solo *metadatos*: carpeta, título y el
último agente visto. **Las terminales vivas no forman parte del modelo**: la interfaz mantiene
una terminal por cada panel de los espacios y pestañas despiertos y las crea o descarta para
que coincidan con el modelo (`sync`). Por eso dormir es barato: el panel sigue en el modelo,
solo se suelta su proceso.

Un espacio o una pestaña tiene un color elegido por el usuario (`Accent`, diez opciones) y los
espacios nuevos reciben uno de una rotación.

## Dormir y despertar

Dormir un espacio (o una pestaña):

1. Se guarda la carpeta actual de cada panel (leída de `/proc/<pid>/cwd`).
2. Se guarda su pantalla e historial como texto con atributos ANSI, comprimido con zstd, en
   `scrollback/<panel>.zst`.
3. Se marca como dormido en el modelo y se persiste.
4. `sync` suelta las terminales, lo que cierra su shell.

Despertar crea un shell nuevo en la carpeta guardada y vuelve a escribir el ANSI guardado
seguido de una marca *session restored*. La pestaña visible de un espacio despierto nunca se
carga dormida, y el último espacio despierto no se puede dormir.

## Persistencia (`mer-store`)

SQLite con migraciones versionadas (`rusqlite_migration`): espacios, pestañas, paneles y una
tabla `meta` para el estado de la interfaz (barra plegada, tamaño de la ventana). Las
escrituras van por un hilo de fondo (`Writer`) que fusiona ráfagas, de modo que la interfaz
nunca espera al disco. El historial son archivos aparte, uno por panel.

## La interfaz (`mer-ui`)

- `TerminalView` es un elemento propio de GPUI que dibuja la cuadrícula: fondos fusionados en
  tramos, texto agrupado por fila y selección. Solo redibuja cuando llega salida del PTY o hay
  entrada del usuario.
- `WorkspaceView` es la vista principal: barra lateral, pestañas, divisiones, diálogos y menús.
  Se crea **una vez**, sin ventana, y las ventanas se acoplan y desacoplan
  ([ADR 0005](adr/0005-view-outlives-window.md)). Nada en ella recibe una `Window`: el foco se
  *pide* y se aplica al dibujar.
- Los comandos (`Cmd`) son el punto común de los atajos, la paleta, los menús y `mer ctl`.
- `settings_panel` edita la misma estructura `Config` que el archivo TOML.

## Agentes (`mer-agents`)

`AgentRegistry` decide qué agente es un proceso; `AgentStatus` es una pequeña máquina de
estados alimentada por los eventos de los hooks. Ver [`agentes.md`](agentes.md).

## IPC (`mer-ipc`)

Un socket Unix en `$XDG_RUNTIME_DIR/mer/mer.sock`, con mensajes JSON de una línea y un campo
de versión (`{"v":1,…}`). Hay tres clases de petición: `Ping`, `Hook` (los agentes) y `Ctl`
(`mer ctl`). Los hooks y las consultas se atienden sin ventana; lo que muestra algo al usuario
abre la ventana primero.

## Configuración (`mer-config`)

`Config` y los temas se leen con `serde`; se guardan con `toml_edit` para conservar los
comentarios. Un observador de archivos (`notify`) recarga la configuración en caliente.

## Decisiones

Registradas en [`adr/`](adr): GPUI para la interfaz (0001), `alacritty_terminal` como motor
(0002), un solo proceso (0003), SQLite (0004) y la vista que sobrevive a su ventana (0005).
