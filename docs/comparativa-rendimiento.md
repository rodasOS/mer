# mer vs Konsole: comparativa de rendimiento (2026-11-21)

> Máquina del mantenedor: AMD Ryzen 7 5700U (16 hilos), 30,7 GiB, Fedora 44, KDE Plasma 6 en Wayland,
> Konsole 26.08.2. Método de memoria: PSS de `/proc/<pid>/smaps_rollup`.
> Cada caso: 3 muestras en la misma sesión, una sola pestaña, mediana.

## Qué se cambió en mer
1. **Heap sin huge pages (THP)**: `crates/mer/src/main.rs` (`prctl(PR_SET_THP_DISABLE)` con `rustix`, sin `unsafe`).
2. **GPU: Vulkan primero, OpenGL solo de respaldo**: copia local de `gpui_wgpu` en `vendor/gpui_wgpu`
   (`[patch]` en `Cargo.toml`, ADR 0007). Ya no se cargan EGL, gallium ni el LLVM de OpenGL.

## Memoria en reposo (ventana abierta, 22 s sin hacer nada)

| Terminal | RSS | **PSS** | Privada | Hilos | CPU en reposo |
|---|---|---|---|---|---|
| Konsole 26.08.2 | 95 MB | **35 MB** | 21 MB | 9–11 | 0 |
| mer instalado antes (0.1.0) | 149 MB | **89 MB** | 65 MB | 37 | 0 |
| mer 0.1.1 genérico (`.rpm`, x86-64-v2) | 93 MB | **55 MB** | 40 MB | 30 | 0 |
| mer 0.1.1 nativo (`target-cpu=native`) | 93 MB | **54 MB** | 40 MB | 30 | 0 |

- mer baja **−38 %** en RSS, PSS y privada respecto al instalado antes.
- Frente a Konsole: **RSS casi igual**, pero ~20 MB más de PSS. Los "40 MB" que ves en Konsole son su PSS (35 MB); el RSS de Konsole (95 MB) incluye Qt compartido con el resto de KDE.
- Qué se ganó por pieza (RSS): 149 → 113 MB solo con THP apagado → 93 MB con Vulkan primero.

## Con carga: `seq 1 3000000` (22 MB de salida) dentro de la ventana

| Terminal | Tiempo | RSS tras la carga | PSS tras la carga |
|---|---|---|---|
| Konsole | 4,79 s | 93 MB | 33 MB |
| mer instalado antes | 2,41 s | 170 MB | 112 MB |
| mer genérico | 2,45 s | 117 MB | 78 MB |
| mer nativo | 2,45 s | 117 MB | 78 MB |

- La velocidad **no cambió** (diferencias de ±2 %, dentro del ruido) y mer sigue siendo ~2× más rápido que Konsole en esta prueba.
- Con carga mer usa más memoria que Konsole (78 vs 33 MB de PSS): el búfer de la GPU y el scrollback de 10 000 líneas por panel.

## Genérico vs nativo
Sin diferencia medible en memoria ni en velocidad. El RPM genérico basta; compilar para tu CPU no aporta nada aquí.

## Paquete `.deb` (Ubuntu 26.04 en distrobox `mer-deb-test`)
- `apt install ./mer_0.1.1_generico-x86-64-v2_amd64.deb` resolvió todas las dependencias y `ldd` no reporta librerías faltantes.
- Abre la ventana, ejecuta el shell y **no carga EGL ni gallium** (el parche funciona también ahí). Vulkan: AMD RADV y llvmpipe visibles.
- Memoria dentro del contenedor: RSS 126 MB, PSS 117 MB (una muestra). **No es comparable** con Fedora: ahí las librerías no se comparten con el resto del sistema.

## Respaldo de GPU (probado)
| Caso | Resultado |
|---|---|
| Normal | Vulkan con la GPU real (RADV), 91 MB RSS |
| Sin Vulkan (`VK_ICD_FILENAMES=/nonexistent`) | abre con OpenGL, 121 MB RSS |
| Solo Vulkan por software (`*lvp*`) | rechaza el adaptador de software y usa OpenGL con la GPU real |

## Pruebas
`cargo test --workspace` (todo en verde), `cargo clippy` y `cargo fmt --check` sin avisos. En el shell de una ventana de mer: `TERM`/`COLORTERM` correctos, `eglinfo` ve las 6 plataformas EGL y `vulkaninfo` 2 dispositivos, sin variables de entorno filtradas.

## Cosas que no se cambiaron y por qué
- **Fuentes, hilos, `opt-level="s"`, scrollback restaurado**: lo que queda en la memoria de mer es sobre todo código del binario compartido (18,6 MB de PSS) y heap real (~18 MB). Fuentes ≈ 2 MB, LLVM de los drivers Vulkan ≈ 5 MB. Cada paso daría menos de ~5 MB con riesgo de regresión.
- Los procesos que lances desde mer **heredan el flag "sin THP"**; en este equipo (THP en `madvise`) no tiene efecto práctico.
- El RPM/`.deb` "genérico" compilado en esta Fedora exige glibc ≥ 2.43. Para distros más viejas hay que compilar en el runner de Ubuntu 22.04 del workflow de GitHub.
- Los números son de una instancia aislada y una sola pestaña; con más pestañas y paneles crece el scrollback.
