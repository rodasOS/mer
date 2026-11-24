# Agentes

`mer` reconoce a los agentes de programación que corren en tus terminales y muestra qué hacen.
Funciona en dos capas: la primera siempre está activa; la segunda da el estado exacto.

## 1. Por el proceso (siempre)

Cuando una terminal produce salida (como mucho cada ~350 ms), `mer` mira qué proceso tiene en
primer plano leyendo `/proc/<pid>/stat` y `/proc/<pid>/cmdline`. Si coincide con una regla,
la terminal queda marcada con ese agente. No usa más CPU que eso y no hace nada mientras la
terminal está quieta.

Reglas incluidas:

| Agente | Se reconoce por |
|---|---|
| Claude Code | `claude`, `@anthropic-ai/claude-code` |
| Codex CLI | `codex`, `@openai/codex` |
| Gemini CLI | `gemini` |
| opencode | `opencode` |

Un nombre simple debe coincidir con el nombre del programa, de modo que `vim claude-notes.txt`
no cuenta como Claude. Para agregar otros agentes usa `[[agents.rules]]` en la configuración
(ver [`configuracion.md`](configuracion.md)).

## 2. Por hooks (estado exacto)

Los hooks avisan a `mer` de lo que hace el agente. Cada terminal de `mer` recibe dos variables
de entorno, `MER_PANE_ID` y `MER_SOCKET`, y el comando `mer hook --agent <nombre>` manda el
evento al socket de la instancia que corre.

| Agente | Evento | Estado en `mer` |
|---|---|---|
| Claude Code | `UserPromptSubmit` | trabajando |
| Claude Code | `PostToolUse` | trabajando (vuelve a este estado tras aprobar un permiso) |
| Claude Code | `Notification` (`permission_prompt`, `elicitation_dialog`) | esperando tu respuesta |
| Claude Code | `Stop` | terminado |
| Claude Code | `SessionStart` | guarda el identificador de la sesión |
| Codex CLI | `notify` (`agent-turn-complete`) | terminado |

Los avisos `idle_prompt` de Claude ("sigo esperando tu mensaje") y `SubagentStop` se ignoran: no
significan que el agente te necesite ni que haya terminado.

Dos casos que ningún hook cubre se resuelven por lo que tecleas en el panel del agente:

- **Esc o Ctrl+C** con el agente trabajando o esperando lo pasa a inactivo (Claude Code no avisa
  al interrumpirlo).
- **Enter** en un panel con Codex lo pasa a trabajando (su `notify` solo avisa al terminar). Es
  una aproximación: un Enter dentro de un diálogo de Codex también cuenta.

Un agente detectado por proceso que aún no ha mandado ningún hook se muestra como *detected*, no
como *idle*, porque su estado real no se conoce.

Con la detección por proceso, Gemini CLI y opencode se muestran sin estado detallado.

### Instalarlos

En **Ajustes → Agents** hay un botón *Preview install…* por agente. Antes de tocar nada
muestra exactamente qué va a cambiar en:

- `~/.claude/settings.json` (Claude Code), o
- `~/.codex/config.toml` (Codex CLI; si ya tienes un `notify` propio, `mer` no lo pisa).

Al aplicar, guarda una copia del archivo original con el sufijo `.bak-mer`. Si el archivo no
es válido, `mer` se niega a modificarlo. Los hooks no hacen nada si `mer` no está corriendo.

### Integración de shell (nombre exacto del comando)

Para el nombre de la pestaña, `mer` mira el proceso en primer plano. Algunos programas
reescriben su nombre (`ionic serve` aparece como `ionic`). Para ver el comando exacto, en
**Ajustes → Agents → Shell integration (zsh)** pulsa *Preview install…*: añade a `~/.zshrc`
(con copia `.zshrc.bak-mer`) esta línea:

```sh
command -v mer >/dev/null && eval "$(mer shell-init zsh)"
```

El fragmento solo actúa dentro de una terminal de `mer` y no lanza ningún proceso: antes de
cada comando manda su texto a `mer` como un título privado (`mer:cmd:…`) que nunca se muestra.
Es solo para zsh.

La misma integración manda, en cada prompt, la carpeta y la versión de node de nvm (otro título
privado, `mer:ctx:…`, sin lanzar procesos: se lee `$NVM_BIN`). Con eso `mer` dibuja bajo la
terminal los chips de **versión de node** y **rama git**. Al pulsar uno se abre una lista con
buscador; al elegir, `mer` escribe `git switch <rama>` o `nvm use <versión>` en el shell.

Si el proyecto tiene un `.nvmrc` (o `.node-version`), el chip de node lo compara con la versión
activa: **verde con ✓** si coinciden, **ámbar** (`v24.15.0 → v18.20.4`) si difieren y **rojo** si la
versión pedida no está instalada (el tooltip dice qué `nvm install` falta). En la lista, la versión
pedida lleva la etiqueta `.nvmrc` y el cursor empieza en ella. Se lee de forma perezosa: solo
cuando cambia la carpeta o la rama, en segundo plano, sin ejecutar `nvm` ni `node`, y puede
apagarse con `terminal.node_version_hint`. Los alias (`lts/*`, `node`) se muestran pero no se
juzgan, porque solo `nvm` sabe resolverlos.

Eso solo ocurre si el shell está esperando en el prompt **y** no hay otro programa en primer plano:
si corre un agente, un editor o un servidor, el chip se atenúa y no hace nada, así que `mer` nunca
manda teclas a un programa en ejecución ni mata ningún proceso. Un servidor que corra en *otra*
pestaña sobre el mismo repositorio sí verá cambiar los archivos, igual que si cambiaras de rama a
mano. Los shells que ya estaban abiertos al actualizar `mer` no tienen el hook de prompt hasta
reiniciarlos. Solo se entiende nvm (no fnm, volta ni asdf).

## Qué ves

- El **nombre de la pestaña** sigue siempre el mismo orden: **1)** el nombre que le pusiste a mano (nada
  lo reemplaza); **2)** el programa que corre (`vite dev`, `claude`) con la carpeta atenuada al
  lado, pero solo cuando lleva unos 0,8 s en primer plano, así un `ls` o un `clear` no parpadean en el
  título; **3)** en reposo, en el prompt, el **nombre de la carpeta** (igual en todas las pestañas);
  **4)** el título que puso el shell, solo si no se conoce la carpeta; **5)** `shell`. Al volver al
  prompt el comando se borra al instante (no depende de ningún hook de título tuyo). La ventana usa
  el mismo nombre que la pestaña. En el sidebar, una terminal suelta muestra ese nombre y debajo la
  carpeta (`~/dev/mer`) con la rama, sin repetir el título. Al pasar el mouse, el tooltip da el
  comando, la carpeta completa y el título del shell.
- Un punto de color junto a la terminal, la pestaña y el espacio: trabajando, esperando o
  terminado.
- La sección **AGENTS** de la barra lateral, con el estado de cada agente y dónde está.
- Una **notificación de escritorio** cuando un agente espera tu respuesta o termina, si `mer`
  no está al frente o ese panel no está a la vista (se puede apagar con `agents.notify`).
  Sigue funcionando con la ventana cerrada si activaste *Keep running*.
- Un **sonido corto** en los mismos casos (`agents.sound`, apagable en Ajustes → Agents, con un
  botón *Test*). Lo reproduce `pw-play` o, si no está, `paplay`. Dos sonidos seguidos en menos
  de 1,5 s se juntan en uno, y no suena con «No molestar» de KDE activo. El volumen
  (`agents.sound_volume`, 50 por defecto) se ajusta en Ajustes → Agents; el botón *Test* suena
  con el volumen elegido.

## Aviso de comando largo

Cuando un comando que tardó `agents.long_command_seconds` (30 por defecto) o más termina y no
estabas mirando ese panel (otra pestaña, otro espacio u otra ventana), `mer` avisa con una
notificación («`cargo build` finished · 2 min 13 s») y el sonido de «terminado». No avisa de
sesiones interactivas (editores, paginadores, shells, `htop`, `ssh`, `tmux`…) ni de agentes,
que tienen sus propios avisos. Se ajusta en Ajustes → Agents o con `long_command_seconds = 0`
para apagarlo.

El inicio se nota cuando el terminal produce salida mientras el comando corre (con la
integración de zsh, al instante). Un comando que corre en silencio desde el principio se mide
desde que aparece su primera salida.

## Reanudar al despertar (opcional)

Con `agents.resume_on_wake = true` (o el interruptor en Ajustes), al despertar un espacio que
tenía a Claude Code, `mer` escribe `claude --resume <sesión>` en esa terminal. Solo se
reanuda un identificador de sesión válido (letras, números, `-` y `_`, hasta 128 caracteres). Está **apagado por
defecto** y solo existe para Claude Code, el único cuyo identificador recibimos.
