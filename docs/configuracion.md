# Configuración

`mer` lee `~/.config/mer/config.toml` (o `$XDG_CONFIG_HOME/mer/config.toml`). Todas las claves
son opcionales: lo que falte toma su valor por defecto, y los valores fuera de rango se
corrigen al cargar. Los cambios en el archivo se aplican al instante, sin reiniciar. Cuando
cambias algo desde el panel de ajustes, `mer` guarda el archivo **conservando tus comentarios**.

Si el archivo tiene un error, `mer` lo ignora, usa los valores por defecto y lo anota en el
registro (`MER_LOG=warn mer`).

## `[appearance]`

| Clave | Por defecto | Descripción |
|---|---|---|
| `theme` | `"mocha"` | Tema. Incluidos: `mocha`, `tokyo-night`, `gruvbox-dark`, `latte`. También los de `~/.config/mer/themes/`. |
| `font_family` | `"JetBrainsMono Nerd Font Mono"` | Fuente de la terminal (monoespaciada). |
| `font_size` | `14` | Tamaño en puntos (8 a 32). El zoom se suma a este valor durante la sesión. |
| `line_height` | `1.35` | Multiplicador de la altura de línea. |
| `ui_font` | `"Inter"` | Fuente de la interfaz (barra lateral, menús, ajustes). |
| `sidebar_side` | `"right"` | Lado de la ventana donde va la barra lateral de espacios: `"left"` o `"right"`. También en Ajustes → Appearance. |
| `ui_scale` | `1.0` | Tamaño de la interfaz (0.8 a 1.6): barra lateral, pestañas, menús y diálogos. No cambia el texto de la terminal, que tiene su propio `font_size` y su zoom. |

## `[terminal]`

| Clave | Por defecto | Descripción |
|---|---|---|
| `shell` | *(automático)* | Ruta del shell. Sin ella: `$MER_SHELL` si existe, si no `zsh` cuando está instalado, si no tu shell de login. Aplica a las terminales nuevas. |
| `scrollback` | `10000` | Líneas de historial por terminal (terminales nuevas). |

## `[spaces]`

| Clave | Por defecto | Descripción |
|---|---|---|
| `auto_sleep_minutes` | `0` | Duerme un espacio que no visitas hace tantos minutos. `0` lo desactiva. Salta los espacios con algo corriendo. |
| `restore_scrollback` | `true` | Muestra el historial anterior de cada terminal al despertar un espacio. |
| `keep_running_on_close` | `false` | Cerrar la ventana deja `mer` y sus terminales corriendo en segundo plano. |

## `[agents]`

| Clave | Por defecto | Descripción |
|---|---|---|
| `notify` | `true` | Notificación de escritorio cuando un agente te necesita o termina y `mer` no está al frente. |
| `sound` | `true` | Un sonido corto en esos mismos casos. No suena con «No molestar» de KDE activo. |
| `sound_waiting` | `"message-attention"` | Sonido cuando el agente te necesita: un nombre del tema de sonidos (`/usr/share/sounds/*/stereo`) o la ruta de un archivo `.oga`, `.ogg` o `.wav`. |
| `sound_done` | `"complete"` | Sonido cuando el agente termina. |
| `sound_volume` | `50` | Volumen de esos sonidos, de 0 a 100 (`0` los silencia). Solo afecta a los sonidos de `mer`: el sonido propio de las notificaciones de KDE se ajusta en Preferencias del sistema → Notificaciones. |
| `long_command_seconds` | `30` | Avisa (notificación y el sonido de «terminado») cuando un comando que tardó al menos tantos segundos termina y no estabas mirando ese panel. `0` lo apaga. Editores, paginadores, shells, monitores y agentes nunca cuentan. |
| `resume_on_wake` | `false` | Al despertar un espacio, vuelve a abrir la sesión del agente (solo Claude Code: `claude --resume <sesión>`). |
| `rules` | *(vacío)* | Reglas propias para reconocer otros agentes (ver abajo). |

Reglas propias de detección:

```toml
[[agents.rules]]
name = "aider"
pattern = "aider"       # un nombre simple debe coincidir con el nombre del programa
```

## `[keys]`

Ver [`atajos.md`](atajos.md).

## Temas propios

Un archivo `~/.config/mer/themes/<nombre>.toml`:

```toml
name = "mi-tema"          # opcional; si falta se usa el nombre del archivo
dark = true               # opcional, por defecto true

[colors]
foreground = "#cdd6f4"
background = "#1e1e2e"
cursor = "#f5e0dc"        # opcional
ansi = [                  # exactamente 16 colores: 8 normales y 8 brillantes
  "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
]

[chrome]                  # opcional: colores de la barra lateral y los menús.
sidebar = "#181825"       # Lo que omitas se deriva de los colores de arriba.
bar = "#11111b"
surface = "#313244"
border = "#313244"
text = "#cdd6f4"
text_dim = "#6c7086"
accent = "#89b4fa"
```

Los colores se escriben en hexadecimal. Un tema con un error se omite y se anota en el registro. Si el `name` coincide con un tema
incluido, lo reemplaza.

## Variables de entorno

| Variable | Efecto |
|---|---|
| `MER_LOG` | Nivel de registro (`warn`, `info`, `debug`). |
| `MER_SHELL` | Shell a usar si `terminal.shell` no está definido. |
| `MER_BENCH` | Imprime el tiempo hasta el primer cuadro (lo usa `cargo xtask bench`). |
| `MER_PANE_ID`, `MER_SOCKET` | Las pone `mer` dentro de cada terminal para que los hooks de los agentes sepan a quién avisar. No las cambies. |
| `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` | Cambian dónde se leen la configuración, el estado y el socket. Útiles para probar con un estado aislado. |
