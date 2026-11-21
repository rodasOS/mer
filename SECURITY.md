# Seguridad

## Cómo reportar un problema

Si encuentras una vulnerabilidad, **no abras un issue público**. Usa el aviso privado de
GitHub ("Report a vulnerability" en la pestaña *Security* del repositorio) o escribe al
autor. Se responde en cuanto se pueda; es un proyecto personal.

## Qué toca `mer` en tu sistema

Para que puedas valorar el riesgo:

- **Socket de control** en `$XDG_RUNTIME_DIR/mer/mer.sock`. Es un socket Unix local, creado
  en tu directorio de ejecución. La carpeta es `0700` y el socket `0600` (si la carpeta ya
  existía más abierta, se corrige al arrancar), así que solo tu usuario puede abrirlo. Quien
  pueda escribir en él puede abrir pestañas, dormir o borrar espacios o cerrar `mer`; no hay
  una orden para ejecutar comandos arbitrarios ni para leer o escribir en una terminal. Cada
  petición está limitada a 1 MiB y cada lectura tiene un tiempo de espera de 2 s.
- **Variables de entorno** `MER_PANE_ID` y `MER_SOCKET` en cada terminal, para que los hooks de
  los agentes encuentren la instancia.
- **Hooks de agentes.** Solo si tú lo pides desde Ajustes, `mer` edita
  `~/.claude/settings.json`, `~/.codex/config.toml` o `~/.zshrc`. Muestra antes los cambios y
  deja una copia `.bak-mer` de tu archivo original (la primera copia no se vuelve a pisar). La
  escritura es atómica, conserva los permisos del archivo (un `settings.json` `0600` sigue
  siendo `0600`) y respeta los enlaces simbólicos. La ruta del ejecutable se escapa para el
  shell. Los hooks solo mandan eventos de estado al socket.
- **Reanudar al despertar** (apagado por defecto) escribe `claude --resume <id>` en una
  terminal; el identificador se valida (letras, números, `-` y `_`) antes de usarse.
- **Regla de KDE** para recordar la posición de la ventana: solo si la activas, y se quita
  desde el mismo botón. Edita `~/.config/kwinrulesrc`.
- **Estado en disco** en `~/.local/state/mer`: la estructura de tus espacios y el historial de
  tus terminales (sin cifrar). La carpeta se crea `0700` y los archivos `0600`, también la
  base de datos y sus archivos `-wal` y `-shm`; lo creado por versiones anteriores se corrige
  al arrancar. Un historial que al descomprimirse supere 1 GiB se descarta. Si en una
  terminal apareció un secreto, también queda en su historial; bórralo con *Delete space…* o
  eliminando `scrollback/`.
- **Portapapeles.** Al pegar, `mer` quita los caracteres de control (Esc, Ctrl+C…) y evita que
  el texto cierre antes de tiempo el "pegado entre corchetes", para que un texto copiado de
  una web no pueda ejecutarse como si lo hubieras tecleado.

`mer` no abre conexiones de red propias (las notificaciones usan el D-Bus local).

## Límites conocidos de seguridad

- Como en la mayoría de terminales, un programa que corre dentro de `mer` puede **escribir** en el
  portapapeles con la secuencia OSC 52 (leerlo no está permitido). Si no confías en lo que
  ejecutas, ten en cuenta que puede dejar texto en tu portapapeles.
- Si el shell configurado no existe o no se puede abrir un PTY, `mer` se cierra con un error en
  lugar de mostrar un aviso.
- Las dependencias se comprueban con `cargo deny` (incluye los avisos de RustSec) en cada
  cambio y cada semana, sin vulnerabilidades reportadas. Algunas librerías de GPUI figuran como "sin mantenimiento"
  y están anotadas en `TODO.md`.
