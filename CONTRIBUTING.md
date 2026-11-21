# Contribuir a mer

`mer` es un proyecto personal, pero las ideas, los reportes de errores y los cambios son
bienvenidos. Antes de un cambio grande, abre un *issue* para hablarlo.

## Preparar el entorno

```sh
git clone git@github.com:rodasOS/mer.git
cd mer
cargo run            # compila y abre mer
```

`rustup` instala sola la versión de Rust que fija `rust-toolchain.toml`. Hace falta Linux con
Wayland y un driver Vulkan (ver el [README](README.md#instalación)).

> **Cuidado al probar:** `mer` guarda su estado en `~/.local/state/mer`. Para no tocar el tuyo,
> usa un estado aislado:
>
> ```sh
> I=/tmp/mer-prueba; mkdir -p $I/state $I/config $I/run
> XDG_STATE_HOME=$I/state XDG_CONFIG_HOME=$I/config XDG_RUNTIME_DIR=$I/run cargo run
> ```
>
> El socket de Unix limita la ruta a ~100 caracteres, así que usa una carpeta corta.

## Antes de enviar un cambio

```sh
cargo xtask ci
```

Corre exactamente lo que corre la integración continua:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

`clippy` va en modo `pedantic`, el código no usa `unsafe` (está denegado) y las advertencias
son errores. Si un lint no tiene sentido en un sitio concreto, usa `#[allow(...)]` con un
comentario que diga por qué.

## Convenciones

- **Idioma:** el código, los identificadores, los comentarios y los ADR van en inglés. La
  documentación para usuarios (README y `docs/`) va en español.
- **Commits:** [Conventional Commits](https://www.conventionalcommits.org) en inglés, en
  imperativo y con menos de 72 caracteres: `feat(ui): add terminal text zoom`. Tipos: `feat`,
  `fix`, `refactor`, `style`, `docs`, `test`, `chore`, `build`. Un commit por cambio lógico.
- **Pruebas:** un cambio de comportamiento lleva su prueba. Los crates `mer-core`, `mer-term`,
  `mer-agents`, `mer-store`, `mer-config` e `mer-ipc` se prueban sin abrir ninguna ventana;
  mantén ahí la lógica y deja en `mer-ui` solo lo que dibuja.
- **Decisiones de arquitectura:** si cambias algo que afecta a varios crates o revierte una
  decisión anterior, escribe un ADR nuevo en `docs/adr/` (formato corto: contexto, decisión,
  consecuencias).

## Dónde tocar

| Quiero… | Dónde |
|---|---|
| Añadir un atajo o acción | `crates/mer-ui/src/actions.rs` (la tabla `binding_table!`), un `Cmd` en `commands.rs` y su caso en `WorkspaceView::run` |
| Añadir un ajuste | el campo en `crates/mer-config/src/config.rs` (con su valor por defecto y test) y una fila en `settings_panel.rs` |
| Añadir un comando de `mer ctl` | `crates/mer-ipc/src/protocol.rs`, `crates/mer/src/cli.rs` y `run_ctl` en `workspace_view/agents.rs` |
| Cambiar el modelo | `crates/mer-core`; si cambia lo que se guarda, una migración nueva en `crates/mer-store/src/db.rs` (nunca edites una migración ya publicada) |
| Reconocer otro agente | `crates/mer-agents/src/registry.rs` |

Para ver una parte de la interfaz sin mouse, `mer ctl show …` abre ajustes, la paleta y los
menús (ver el README).

## Medir rendimiento

`mer` se compara contra kitty y ghostty. `cargo xtask bench` mide y reescribe
`docs/performance.md`; `--quick` es una versión corta **que no es comparable** con la
completa, así que no commitees su salida sobre los números reales.
