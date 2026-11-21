# TODO

Pendientes que no se hicieron a propósito. Cada uno tiene un costo que se prefirió no pagar todavía.

## Guardado agrupado (~500 ms)

- **Qué:** el escritor de SQLite (`crates/mer-store/src/writer.rs`) junta los cambios durante ~500 ms
  antes de escribir, y no escribe si nada cambió.
- **Por qué:** hoy cada clic de foco, cambio de pestaña o hook reescribe todas las filas
  (`db.rs`, `save`) y hace un `readlink` de `/proc/<pid>/cwd` por panel en el hilo de la interfaz.
- **Costo:** si `mer` se cae (no al cerrar normal), se pierde el último medio segundo de cambios.

## Redimensionado diferido

- **Qué:** `paint_grid` (`terminal_view.rs`) llama a `resize` en cada frame; con 1 M de líneas de
  scrollback, cada cambio de columnas reacomoda todo. Esperar ~40 ms tras el último cambio de
  tamaño antes de reacomodar.
- **Costo:** durante el arrastre del borde de la ventana el texto no se reacomoda en vivo.

## Descartados por poca ganancia o riesgo (sin sacrificar nada, pero no vale la pena aún)

- **Vistas cacheadas** (`Entity::cached`): cada lote de salida vuelve a ejecutar todo
  `WorkspaceView::render`. Cachear la terminal exige notificar a mano el cambio de foco y de
  ajustes; un `notify` olvidado deja la interfaz vieja. Medir antes de hacerlo.
- **Descomprimir el historial (zstd) fuera del hilo de la interfaz** al restaurar: cambia el orden
  en que se crean las terminales.
- **Temporizador de auto-dormir** de 1 minuto: corre aunque la opción esté en 0; la ganancia es
  un despertar por minuto.

## Port a Windows

- **Qué:** que `mer` compile y corra en Windows, para que `release-windows.yml` pueda publicar
  un `.exe`. Hoy falla al compilar.
- **Trabajo:** IPC (`mer-ipc`) con named pipes en vez de sockets Unix; detección de agentes
  (`mer-agents/src/process.rs`) sin `/proc` (p. ej. `sysinfo`); directorio del shell
  (`mer-term/src/session.rs`) por OSC 7 en vez de `/proc/<pid>/cwd`; rutas de `mer-config`
  con `%APPDATA%`; sonido sin `pw-play`/`paplay`; integración de shell para PowerShell; quitar
  KWin y `busctl` con `cfg`; un instalador (WiX o Inno Setup).
- **Costo:** semanas de trabajo, y una máquina Windows para probar. ConPTY añade algo de
  latencia y CPU frente a un PTY de Unix. Hay que abstraer varias piezas detrás de `cfg` en
  crates que hoy asumen Unix, y escribir un ADR que revoque "solo importa esa máquina".

## Dependencias con aviso de "sin mantenimiento"

- **Qué:** `cargo audit` no reporta vulnerabilidades, pero marca como sin mantenimiento `chrono`,
  `paste`, `rustybuzz`, `ttf-parser`, `zbus_names`, `zvariant` y `zvariant_derive`. Todas las trae
  GPUI (o sus dependencias), no `mer`.
- **Por qué no se arregla:** GPUI está fijado a un commit (ADR 0001) y no se actualiza sin pedirlo.
  Al subirlo, volver a ejecutar `cargo deny check` y revisar si esos avisos desaparecen.
- **Costo:** ninguno hoy; si aparece una vulnerabilidad en alguna, habría que subir GPUI de golpe.

## `rusqlite` 0.41 (hay 0.43)

- **Qué:** `rusqlite_migration` 2.7.0, la última publicada, exige `rusqlite ^0.41`.
- **Qué hacer:** subir ambas juntas cuando salga una `rusqlite_migration` compatible con 0.43.
