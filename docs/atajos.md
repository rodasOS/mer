# Atajos de teclado

Todos los atajos se pueden cambiar en la tabla `[keys]` de `~/.config/mer/config.toml` (o ver
la lista en **Ajustes → Shortcuts**). El nombre de la acción va a la izquierda y la tecla a la
derecha; una cadena vacía desactiva el atajo. Varias teclas para una misma acción se separan
con `|`:

```toml
[keys]
new_tab = "ctrl-t"
zoom_in = "ctrl-= | ctrl-+"
sleep_space = ""            # sin atajo
```

Las teclas se escriben como `ctrl-shift-t`, `alt-shift-left`, `ctrl-tab`… (en minúsculas y
unidas con `-`).

## Por defecto

| Acción (`[keys]`) | Atajo | Qué hace |
|---|---|---|
| `new_tab` | `ctrl-shift-t` | Pestaña nueva |
| `close_pane` | `ctrl-shift-w` | Cierra el panel enfocado |
| `split_right` | `ctrl-shift-d` | Divide hacia la derecha |
| `split_down` | `ctrl-shift-e` | Divide hacia abajo |
| `new_space` | `ctrl-shift-n` | Espacio nuevo |
| `next_tab` / `prev_tab` | `ctrl-tab` / `ctrl-shift-tab` | Pestaña siguiente o anterior |
| `next_space` / `prev_space` | `ctrl-shift-pagedown` / `ctrl-shift-pageup` | Espacio siguiente o anterior (despierta si dormía) |
| `focus_left` `focus_right` `focus_up` `focus_down` | `ctrl-shift-` + flecha | Mueve el foco entre paneles |
| `resize_left` `resize_right` `resize_up` `resize_down` | `alt-shift-` + flecha | Mueve el divisor del panel |
| `toggle_sidebar` | `ctrl-shift-b` | Oculta o muestra la barra lateral (oculta, no deja ninguna franja; el botón de arriba también la vuelve a mostrar) |
| `new_terminal` | *(sin tecla)* | Crea una terminal suelta, fuera de cualquier espacio. `new_tab` dentro de una terminal también crea otra terminal (una terminal suelta no tiene barra de pestañas) |
| `open_palette` | `ctrl-shift-p` | Paleta de comandos |
| `open_settings` | `ctrl-,` | Ajustes |
| `sleep_space` | `ctrl-shift-z` | Duerme el espacio actual |
| `zoom_in` | `ctrl-=` `ctrl-+` `ctrl-shift-=` | Acerca el texto (varias teclas para cubrir distintos teclados) |
| `zoom_out` | `ctrl--` | Aleja el texto |
| `zoom_reset` | `ctrl-0` | Restablece el tamaño |
| `move_tab_left` / `move_tab_right` | *(sin tecla)* | Mueve la pestaña activa un lugar |
| `move_space_up` / `move_space_down` | *(sin tecla)* | Mueve el espacio activo un lugar |
| `search` | `ctrl-shift-f` | Abre la búsqueda en el historial de la terminal enfocada |
| `scroll_to_bottom` | `ctrl-shift-end` | Vuelve al final del historial de la terminal enfocada |
| `ui_scale_up` | `ctrl-alt-=` `ctrl-alt-+` `ctrl-alt-shift-=` | Agranda la interfaz (no la terminal) |
| `ui_scale_down` | `ctrl-alt--` | Achica la interfaz |
| `ui_scale_reset` | `ctrl-alt-0` | Tamaño normal de la interfaz |
| `quit` | `ctrl-shift-q` | Cierra `mer` y todo lo que corre en él |

Sin atajo por defecto (se pueden asignar o usar desde la paleta): `close_tab`, `close_space`,
`rename_space`, `rename_tab`, `focus_next`, `focus_prev`, `sleep_others`.

El zoom dura mientras `mer` está abierto: no se guarda y no cambia el tamaño de letra de los
ajustes. `Ctrl` + rueda del mouse sobre una terminal también hace zoom.

El tamaño de la **interfaz** (barra lateral, pestañas, menús) es aparte: se cambia con
`Ctrl+Alt+=` y `Ctrl+Alt+-`, o en Ajustes → Appearance, y sí se guarda (`appearance.ui_scale`).

> **Ojo:** `Ctrl+-`, `Ctrl+=` y `Ctrl+0` ya no llegan al programa de la terminal, porque los
> atajos de `mer` se resuelven antes. Si necesitas esas teclas (por ejemplo `Ctrl+-` como
> "deshacer" en bash o zsh), cámbialas o desactívalas en `[keys]`.

## Fijos (no configurables)

| Atajo | Qué hace |
|---|---|
| `Ctrl+Shift+C` | Copiar la selección |
| `Ctrl+Shift+V` | Pegar |
| Botón del medio | Pega la selección primaria |
| Doble clic / triple clic | Selecciona una palabra / una línea |
| `Shift` + mouse | Selecciona aunque el programa esté capturando el mouse (vim, htop…) |
| `Shift` + clic derecho | Abre el menú contextual aunque el programa capture el mouse |
| `Ctrl` + rueda | Zoom |
| `Shift+Enter` | Salto de línea en Claude Code y otros agentes (envía `ESC CR`, como Alt+Enter) |
| `Esc` | Cierra un diálogo, un menú o los ajustes |

## Búsqueda en el historial

`Ctrl+Shift+F` abre una barra arriba a la derecha de la terminal enfocada. Mientras escribes se
resaltan los resultados que ves (en amarillo, y en el color de acento el actual) y se salta al
más reciente. El contador dice cuántos hay en todo el historial (hasta 999).

| Tecla | Qué hace |
|---|---|
| `Enter` o ↑ | Va al resultado anterior (más arriba, más antiguo); da la vuelta al llegar al principio |
| `Mayús+Enter` o ↓ | Va al resultado siguiente (más abajo) |
| `Esc` o × | Cierra la barra y devuelve el foco a la terminal |

El texto se busca tal cual (un `.` es un punto, no «cualquier letra»). No distingue mayúsculas de
minúsculas, salvo que lo que escribas tenga alguna mayúscula.
