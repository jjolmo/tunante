# Tunante en postmarketOS (Poco X3 NFC) — plan de proyecto futuro

> Estado: **histórico, superado (2026-09-02).** Escrito el 2026-08-15 tras
> medir el acoplamiento del código de entonces. Lo que aquí se llama «Si
> Tauri no encaja» es exactamente lo que acabó pasando, y no solo en el
> teléfono: `tunante-mini` (Slint) se construyó, creció hasta ser la app de
> escritorio completa, y la app Tauri fue borrada — véase
> `plan-desktop-slint.md`. Las rutas `src-tauri/` que se citan ya no existen;
> el reparto núcleo/UI que este documento pedía es hoy `crates/`. Se conserva
> como registro de la investigación.

## Objetivo

Ejecutar Tunante **nativamente** en un Poco X3 NFC con postmarketOS + KDE Plasma,
tratando el móvil como un ordenador más.

Tres restricciones que marcan todas las decisiones:

1. **Nada de remotos.** No es un mando a distancia ni un servidor al que se
   conecta un navegador. Es la app corriendo en el dispositivo, con su audio
   saliendo por sus altavoces.
2. **El backend se reutiliza.** El motor que decodifica formatos de consola es
   lo que hay que preservar; es el valor del proyecto.
3. **Una sola interfaz para ambos lados.** Idealmente el mismo código de UI
   adaptándose a la pantalla, al estilo de Telegram Desktop, no dos frontends.

## Hardware y sistema

| | |
|---|---|
| Dispositivo | Poco X3 NFC (nombre en clave `surya`) |
| SoC | Snapdragon 732G, ARM64 |
| Sistema | postmarketOS (base **Alpine** → **musl**, no glibc) |
| Escritorio | KDE Plasma |

⚠️ **Alpine usa musl.** Es el dato que más condiciona el plan y conviene tenerlo
presente desde el primer minuto (ver "El problema del binario").

## Lo que ya sabemos, medido (2026-08-15)

Esto no son suposiciones, se comprobó sobre el repo:

```
audio/, metadata/, db/   → 0 referencias a Tauri
commands/ + lib.rs       → 61 comandos, todo el acoplamiento concentrado aquí
frontend                 → 94 llamadas a invoke() en 18 ficheros
```

**El motor ya es portable.** Todo lo que decodifica y reproduce (rodio,
vgmstream, los emuladores de GBA/NSF/SPC/PSF) no sabe que Tauri existe: la única
referencia en esas carpetas está en un fichero de tests. Llevarlo a otro sitio no
es una migración, es enlazarlo desde otro binario.

**La CI ya compila ARM64**, pero solo glibc:

```yaml
- arch: amd64  → ubuntu-22.04
- arch: arm64  → ubuntu-22.04-arm     # produce .deb y .AppImage (glibc)
```

## El problema del binario

Los artefactos ARM64 que publica la CI hoy (`.deb` y `.AppImage`) están
enlazados contra **glibc**. postmarketOS es Alpine, que usa **musl**. Salvo
sorpresa, **no arrancarán tal cual**.

Dos salidas, a evaluar en este orden:

1. **Compilar en el dispositivo** (o en un contenedor Alpine aarch64) con las
   dependencias de los repos de Alpine. Es la vía más directa para la primera
   prueba: si compila y arranca, ya sabemos que el camino existe.
2. **Añadir un target musl a la CI** (`aarch64-unknown-linux-musl`) para producir
   un paquete instalable. Esto solo tiene sentido *después* de que la opción 1
   demuestre que funciona; montarlo antes es trabajo a ciegas.

⚠️ El paquete de la CI pesa 11 MB en `.deb` pero lleva dentro vgmstream (700+
formatos) y varios emuladores completos. Compilar eso en un Snapdragon 732G va a
ser lento; el contenedor Alpine aarch64 en una máquina potente es probablemente
mejor idea que compilar en el móvil.

## Las tres incógnitas a resolver ANTES de escribir código

En este orden. Si la primera falla, el resto no importa.

### 1. ¿Hay WebKitGTK para aarch64 en postmarketOS?

Tauri v2 en Linux renderiza con **WebKitGTK**. Sin él no hay app.

```sh
apk search webkit2gtk
apk info -e webkit2gtk-4.1
```

Alpine tiene el paquete, pero **hay que confirmar que está para aarch64 en la
rama que use ese postmarketOS**. Ojo con el detalle de que Plasma es Qt: que el
escritorio sea KDE no implica que WebKitGTK esté instalado, solo que se puede
instalar.

Si no estuviera disponible, la alternativa es cambiar de motor de ventana (ver
"Si Tauri no encaja").

### 2. ¿Arranca el binario?

```sh
ar x tunante_*_arm64.deb && tar xf data.tar.*
ldd ./usr/bin/tunante          # qué falta
./usr/bin/tunante              # ¿arranca?
```

Con musl es esperable que `ldd` se queje; sirve igual para ver el inventario de
dependencias reales.

### 3. ¿Suena?

rodio saca audio por ALSA o PulseAudio. postmarketOS suele traer PipeWire con
capa de compatibilidad. Comprobar que hay salida antes de dar nada por bueno.

## La interfaz: enfoque híbrido

La UI actual es un clon de foobar2000 pensado para ratón y monitor: columnas
redimensionables, menú contextual, middle-click, arrastrar y soltar. En una
pantalla de móvil, buena parte de esas acciones **no tienen gesto equivalente**.

El objetivo es **una sola base de código que se adapte**, como hace Telegram
Desktop, no un segundo frontend. Con Svelte 5 eso significa:

- **Puntos de corte por ancho**, no por sistema operativo. Así el modo compacto
  se prueba en el escritorio encogiendo la ventana, sin necesidad del móvil.
- **Navegación por vistas en pantalla estrecha**: en vez de barra lateral +
  lista + detalle a la vez, una vista cada vez con navegación entre ellas.
- **Equivalentes táctiles** para lo que hoy solo existe con ratón:
  - middle-click (encolar) → pulsación larga o acción en un menú
  - clic derecho → pulsación larga
  - arrastrar a playlist → modo selección + acción
  - columnas redimensionables → ocultas en compacto, con una fila resumen
- **Zonas de toque** de tamaño suficiente. Las de escritorio se quedan cortas.

Esto es trabajo de frontend puro y **se puede hacer y probar sin el móvil**, que
es lo que lo hace un buen primer paso: no bloquea con las incógnitas de arriba.

## Si Tauri no encaja

Si WebKitGTK resulta inviable en ese sistema, el núcleo Rust sigue sirviendo y
solo cambia la capa de ventana. Opciones, sin orden de preferencia:

- Otro binding de webview más ligero, manteniendo el frontend Svelte.
- Interfaz nativa en Qt/QML, que encaja con Plasma pero **rompe el requisito de
  una sola interfaz** — sería un segundo frontend que mantener.

La segunda contradice el objetivo, así que solo entraría en juego si la primera
no existe. Conviene no llegar ahí sin haber agotado el paso 1.

## Fases propuestas

| Fase | Qué | ¿Necesita el móvil? |
|---|---|---|
| 0 | Verificar las tres incógnitas | **Sí** |
| 1 | Compilar en contenedor Alpine aarch64 | No |
| 2 | UI adaptable por ancho, probada en escritorio | No |
| 3 | Instalar y probar en el dispositivo | **Sí** |
| 4 | Ajustes táctiles con el aparato en la mano | **Sí** |
| 5 | Target musl en la CI, si el resto cuajó | No |

Las fases 1 y 2 se pueden adelantar sin acceso al dispositivo. La 0 no.

## Decisiones abiertas

- **¿Plasma Mobile o Plasma de escritorio?** Cambia bastante qué esperar de la
  gestión de ventanas y del escalado.
- **¿Dónde vive la biblioteca?** ¿Música en el propio móvil, en tarjeta SD, o
  montada por red? Afecta al escaneo y a las rutas de los `_ratings.m3u`.
- **¿Se sincronizan los ratings con las otras máquinas?** Si sí, el ajuste de
  prioridad de ratings tendría que ir a `folder` en ese dispositivo (ver
  `Settings → Library → Rating storage priority`).
- **¿Merece la pena el target musl en la CI** o basta con compilar a mano cuando
  toque? Depende de si esto acaba siendo de uso diario o un experimento.

## Referencias en el código

| Qué | Dónde |
|---|---|
| Núcleo portable | `src-tauri/src/audio/`, `src-tauri/src/metadata/`, `src-tauri/src/db/` |

> Las rutas de esta tabla son las de agosto de 2026, antes de que el árbol se
> reorganizara en `apps/` + `crates/` + `vendor/`. El núcleo portable acabó en
> `crates/tunante-core` y `crates/tunante-codec`; la capa de Tauri, en
> `apps/desktop/src-tauri/`.

| Capa acoplada a Tauri | `src-tauri/src/commands/`, `src-tauri/src/lib.rs` |
| Frontend | `src/lib/components/`, `src/lib/stores/` |
| Matriz de compilación | `.github/workflows/release.yml` |
| Parche ARM de viogsf | mismo workflow, paso `Patch viogsf for ARM build` |
