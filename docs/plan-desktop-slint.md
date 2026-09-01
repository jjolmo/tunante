# Tunante desktop sobre el stack de mini — plan de migración

> Estado: **Fase 0 casi cerrada y el punto 1 de la Fase 1 ejecutado el mismo
> día de escribirlo (2026-09-01).** `crates/tunante-audio` existe: el
> `AudioEngine` del desktop decodificando por `DspSource<PipeSource>` — el
> matrimonio quedó consumado. El desktop Tauri ya corre sobre él (rodio dejó
> de ser dependencia directa suya), el decoder viaja como sidecar de Tauri
> (`externalBin` + `scripts/stage-decoder.mjs`, con placeholder en `build.rs`
> para que `cargo check` no lo exija), y el protocolo del decoder ganó
> `--vgm-loops`, el único mando que no podía viajar por la tubería. Verificado:
> workspace y Android en verde, smoke test de formatos en release, y ogg + NSF
> sonando de verdad por la cadena nueva (4,5 MB de pico de RAM en el proceso
> del player con el core NDS-class fuera). El punto 2 también: la maquinaria
> del watcher (backend por plataforma, fallback a polling, debounce) vive en
> `tunante_helper::watch` con el filtro y el manejador inyectados; en el
> desktop queda solo lo que es suyo — releer en proceso con sus scan opts y
> el emit de Tauri — así que hoy no cambia ningún comportamiento y la app
> Slint enchufará `probe` en ese mismo hueco. Y el punto 3: `src/services/`
> tiene los cuerpos de los seis dominios como funciones libres sobre
> `&AppState` + `Events` (el enum tipado de los 13 eventos vive en
> `src/events.rs`, y su adaptador Tauri es el único sitio donde los nombres
> siguen siendo strings); `commands/` quedó en cáscaras de una línea
> (commands 637 líneas, services 2.633). Los cuerpos se movieron verbatim —
> mismos locks, mismo orden, mismos hilos. Quedan dos wrappers de
> compatibilidad (`scan_folder_sync`, `play_with_fade*`) para lib.rs y
> shortcuts.rs, que caen cuando el bootstrap se reescriba en fase 3d.
> Pendiente de la fase: el crate de soporte (4), que según la regla del repo
> espera al segundo consumidor.
>
> **Fase 2, hito de cierre alcanzado (mismo día).** `app.slint` está troceado
> (`types` / `picker` / `widgets` / `tabs` / `desktop` / `app`, cuerpos
> verbatim), y existe el tercer modo de presentación: `ui-mode` (0 auto · 1
> mini · 2 desktop, auto corta en 900px de ancho, mismo patrón snapshot
> anti-ciclo que `portrait`). `DesktopShell` es sidebar (modos + listas +
> Ajustes) | biblioteca | cola, sobre una barra de transporte con seek y
> volumen — construida instanciando `LibraryTab`, `QueueTab` y `SettingsTab`
> enteros, mismos modelos y mismos callbacks: una sola implementación de cada
> comportamiento, cero Rust nuevo salvo el flag `--desktop`/`--mini`.
> Verificado en pantalla con una biblioteca real: ventana ancha → tres
> paneles, cola poblada y un NSF sonando; ventana estrecha → mini exacto,
> sesión restaurada incluida. Rejilla a 5 columnas en desktop.
>
> **Y el arranque de la 3b, mismo día: la tabla existe.** `ui/table.slint` +
> vista «Pistas» en el sidebar (la por defecto del modo desktop): toda la
> biblioteca en columnas # / Título / Artista / Juego / Consola / Duración,
> orden por cabecera con flecha, filtro con plegado de acentos, zebra, hover,
> marca ▶ en la pista sonando, doble click reproduce y el orden visible
> (filtrado+ordenado) se convierte en la cola. Rust posee orden y filtro
> (`TableState` en main.rs, modelo perezoso vía `table-needed` para que el
> teléfono nunca lo pague); verificada en pantalla con metadatos reales.
> Queda de 3b: más columnas y configurables, click derecho/teclado/
> multiselección, ratings, watcher en la app nueva, y el control de
> `ui-mode` en Ajustes.
> Resultados medidos en el portátil (Wayland, panel a ~75 Hz):
>
> - **Spike 1 (la tabla): pasa con nota.** `apps/mini/examples/table_spike.rs`,
>   30.000 filas × 17 columnas. femtovg 75 fps clavados y renderer software
>   69–74 fps — ambos al techo del panel con toda la ventana sucia por frame.
>   Ordenar una columna con el patrón real (sort + reconstruir el modelo
>   entero) cuesta 11–21 ms. Construir los 30k modelos de fila, ~30 ms.
> - **Spike 2 (tray): pasa.** `tray-icon` parcheado construido en un hilo GTK
>   dedicado (`gtk::init` + `gtk::main` fuera del hilo winit): el item SNI
>   queda registrado en el StatusNotifierWatcher y los canales del crate llegan
>   al hilo principal. Falta probar click de menú con un humano delante y el
>   scroll-volumen, pero la arquitectura vale.
> - **Spike 3 (DnD externo): resuelto por lectura de código.** El escape hatch
>   existe (`slint::winit_030::WinitWindowAccessor::on_winit_window_event`,
>   feature `unstable-winit-030`), y winit 0.30 entrega `DroppedFile` en X11,
>   macOS y Windows — **pero no en Wayland** (no está implementado en winit).
>   En Wayland el fallback es el botón «añadir carpeta» que ya existe, o
>   implementar el data_device de Wayland a mano (no lo vale de entrada).
> - **Spike 4 (atajos globales/ratón): pendiente**, riesgo bajo — evdev y
>   CGEventTap no dependen de ningún loop, y `global-hotkey` es lo que el
>   plugin de Tauri usa por debajo.
>
> Objetivo final: **una sola app de escritorio en Slint**. El desktop actual
> (Tauri v2 + SvelteKit) desaparece, `tunante-mini` desaparece como app
> separada, y el binario resultante es el mismo en un monitor de 27" y en el
> teléfono: la diferencia es un modo de presentación, no un programa distinto.

## La decisión de dirección: mini crece, el desktop no se traduce

Hay dos maneras de plantear esto y solo una es sensata.

La tentación es «portar el desktop a Slint»: coger las 13.400 líneas de
Svelte/TS/CSS y traducirlas pantalla a pantalla. Es la vía mala. De esas
13.400 líneas, ~4.000 son CSS que no migra, y una parte grande del resto es
*plumbing* que el nuevo stack elimina de raíz: 79 comandos IPC, 13 eventos,
8 stores con caché y guardias anti-stale que solo existen porque el frontend
vive en otro proceso que el estado. En Slint la UI y el estado comparten
proceso y lenguaje: la mayoría de esos comandos no se traducen — **se
disuelven** en llamadas de función.

La vía buena: **`apps/mini` es el embrión de la app final y se le hace
crecer** hasta la paridad, mientras el desktop Tauri sigue funcionando
intacto. Mini ya tiene, funcionando y probadas en hardware, las piezas más
estructurales: biblioteca con 5 vistas, cola con reorden, playlists completas,
búsqueda FTS5, carátulas con caché, escaneo en hilo, sesión persistente,
MPRIS, y el patrón de arquitectura entero (callbacks planos, `VecModel`,
timer de 500 ms drenando canales, un solo `refresh_library`). Eso son años de
decisiones ya tomadas que una traducción del desktop tiraría.

Al final del plan, `apps/mini` se renombra a la app principal, el directorio
Tauri se borra, y «modo mini» es lo que mini ya hace hoy cuando la ventana es
estrecha.

## Punto de partida (medido, no estimado)

| Pieza | Líneas | Destino |
|---|---|---|
| Frontend Svelte/TS/CSS del desktop | 13.394 | Se tira; su *comportamiento* se reimplementa en Slint |
| `src-tauri/src` exclusivo del desktop | 6.213 | ~2/3 se rescata refactorizado, 1/3 (bootstrap Tauri) se tira |
| `apps/mini` (.rs + .slint) | 6.800 | **Base de la app final** |
| Crates compartidos (core, codec, art, helper, decoder) | ~17.900 | No se tocan (salvo mudanzas *hacia* ellos) |

Del backend del desktop, lo portable ya está identificado módulo a módulo:

- `audio/engine.rs` (546 ln) — **cero Tauri**. Selección de dispositivo,
  reconexión ante desconexión/BT, fade, DSP. Se muda a un crate.
- `watcher/mod.rs` (241 ln) — solo toca Tauri en el `emit` final; un
  `Sender<WatcherEvent>` lo desacopla en ~20 líneas.
- `tray_icon_style.rs` (306 ln), `updater.rs` (315 ln), `debug_log.rs`
  (96 ln), la mitad de `shortcuts.rs` — portables casi tal cual.
- `commands/*` (2.917 ln) — la lógica de negocio es portable; hay que
  extraer los cuerpos de las cáscaras `#[tauri::command]`.
- `lib.rs` (1.428 ln) — lo único realmente atado a Tauri. El hilo de polling
  y el auto-avance de cola se rescatan; el bootstrap se reescribe.

## Dos decisiones técnicas que condicionan todo lo demás

### Motor de audio: fuera de proceso + DSP, lo mejor de cada casa

Hoy conviven dos motores. El desktop decodifica **en proceso**
(`tunante_codec::open_source`) con la cadena DSP de `tunante-core` encima;
mini decodifica **fuera de proceso** (`tunante-decoder` vía
`tunante_helper::PipeSource`) sin DSP.

La app final usa el modelo de mini con el DSP del desktop: `PipeSource`
implementa `rodio::Source` y `DspSource<S>` envuelve cualquier `Source`, así
que `DspSource<PipeSource>` **compone sin obstáculo estructural** — nadie lo
ha escrito aún, pero las dos mitades ya existen y se prueban por separado.
Lo que se gana al quedarse con el modelo fuera de proceso:

- Un core C colgado o corrupto mata al hijo, no a la app. El escaneo es
  interrumpible de verdad (el timeout en proceso del desktop no puede parar
  un bucle en C).
- Teardown instantáneo y memoria devuelta al cambiar de pista (un core NDS
  son ~43 MB que desaparecen al morir el proceso).
- `tunante-codec` con todos los cores vendorizados **deja de enlazarse en la
  app**: solo lo enlaza `tunante-decoder`. Binario más pequeño, compilación
  de la app mucho más rápida.

Lo que hay que añadirle al player de mini para estar a la altura del engine
del desktop: `OutputSelection` (system/device por nombre), enumeración de
salidas, `rebuild_output`/`reconcile_output` (recuperación de desconexiones),
y la cadena DSP. Todo eso está escrito en `audio/engine.rs`; es mudanza, no
invención. La detección pactl de mini (`output.rs`) se conserva como capa
Linux extra.

**Dónde vive**: crate nuevo `crates/tunante-audio` — rodio + `tunante-helper`
+ `tunante-core/dsp`. Ni core (que no debe saber de procesos) ni helper (que
no debe saber de rodio-sink ni de dispositivos) son su sitio.

### Base de datos: una sola, la del desktop

Las dos apps usan el mismo esquema de `tunante-core` en ficheros distintos
(`tunante-mini/tunante-mini.db` vs la del desktop) — separadas a propósito
mientras eran apps distintas. La app final abre **la del desktop** (ahí están
las ~29.500 pistas, los ratings, las clasificaciones y los overrides). Las
claves de sesión de mini ya van namespaced (`mini.*`), así que conviven en la
misma tabla `settings` sin choque. Primer arranque tras la migración: si
existe la BD de mini y la del desktop no, se adopta; si existen ambas, se usa
la del desktop y se importan de la de mini solo las claves `mini.*`.

## Fase 0 — Spikes: matar los riesgos antes de construir encima

Cuatro incógnitas donde Slint/winit puede no dar lo que Tauri daba gratis.
Cada una es un experimento de un día, no una feature; si alguna falla, cambia
el plan y hay que saberlo *antes*.

1. **La tabla.** El componente crítico de toda la migración: la TrackList del
   desktop son 836 líneas resolviendo 17 columnas configurables y
   reordenables, orden por columna, multiselección con teclado, context menu
   y drag hacia el sidebar, virtualizada sobre 29.500 filas. El `ListView` de
   mini virtualiza (filas de altura fija), pero nadie ha probado en Slint una
   tabla de columnas dinámicas a ese tamaño. Spike: 30k filas × 17 columnas,
   scroll, sort, resize de columnas, multiselección. Medir fps en el portátil
   *y* con renderer software.
2. **Tray sin GTK de anfitrión.** El tray del desktop (crates parcheados
   `tray-icon`/`libappindicator`, filtro D-Bus SNI, scroll-para-volumen) vivía
   sobre el bucle GTK que Tauri arrancaba. Mini es winit puro. El camino
   conocido es un hilo dedicado con su propio `gtk::main()` para el tray +
   canal hacia el hilo de UI; el spike confirma que los parches vendorizados
   funcionan así (menú, tooltip, icono simbólico, scroll) en KDE y GNOME.
3. **Drag & drop desde el SO.** Soltar una carpeta para crear playlist. Winit
   entrega `DroppedFile`; comprobar qué expone Slint 1.17 y, si no llega, usar
   el escape hatch de `i-slint-backend-winit` (acceso al evento winit crudo).
   El DnD *interno* (pistas → playlist) no preocupa: mini ya reordena
   arrastrando y no hay WebKitGTK de por medio — el workaround
   `trackDnd.svelte.ts` muere sin sucesor.
4. **Atajos globales y ratón.** El plugin de Tauri usa el crate
   `global-hotkey` por debajo; usarlo directo. Los botones de ratón
   (evdev/CGEventTap) no dependían de Tauri — solo comprobar que conviven con
   el event loop de winit.

## Fase 1 — Mudanzas de backend (el desktop Tauri sigue funcionando)

Todo lo de esta fase mantiene las dos apps compilando y verdes. Es la fase
que ejecuta, otra vez, la lección que este repositorio ya aprendió: cuando
una segunda app necesita algo, se baja al crate, no se copia.

1. `crates/tunante-audio`: nace con el `AudioEngine` del desktop, sustituyendo
   `open_source` por `PipeSource` + `DspSource`. El desktop Tauri **se
   conmuta a este crate ya** — así el motor unificado se prueba meses en la
   app vieja antes de que la nueva dependa de él. El smoke test de formatos
   (`cargo test -p tunante-codec --release`) sigue siendo el listón.
2. `watcher` → desacoplado de Tauri (canal en vez de `emit`) y movido junto a
   `scan` en `tunante-helper` o a un módulo de `tunante-audio`… no: su sitio
   natural es `tunante-helper`, que ya posee el escaneo — el watcher es «el
   escaneo que no termina nunca».
3. `commands/*`: extraer los cuerpos a funciones libres sobre `&AppState` +
   un canal de eventos tipado (el enum que hoy son 13 strings de evento
   Tauri). Las cáscaras `#[tauri::command]` quedan de una línea. Este refactor
   es mecánico pero es **la** garantía de que la lógica de negocio llega a la
   app Slint idéntica, no reescrita de memoria.
4. `updater`, `debug_log`, `tray_icon_style`, el parseo de atajos: a un crate
   de soporte de escritorio (`crates/tunante-desktop-support` o directamente
   dentro de la nueva app si nadie más los quiere — regla del repo: no bajar
   al crate hasta que haya segundo consumidor).

**Al cerrar la fase**: `cargo ndk … check -p tunante-android` obligatorio —
esta fase toca structs compartidos y Android es el sitio donde eso revienta
sin ser mirado.

## Fase 2 — La shell de escritorio en mini

Mini gana su segundo modo de presentación. Hoy ya conmuta por orientación
(portrait/landscape con flags snapshotted — el patrón anti-ciclo documentado
en `app.slint:2689`); se añade la tercera pata: **modo desktop** cuando la
ventana es ancha (umbral ~900 px, con override manual en Ajustes: el «modo
mini» explícito que pide el objetivo del proyecto).

- Trocear `app.slint` (2.978 líneas) en módulos por pantalla antes de crecer:
  `theme.slint` ya existe; salen `tabs/`, `widgets/`, `desktop/`.
- La shell desktop replica el layout del Tauri: sidebar redimensionable
  (All Tracks / Queued / Faved / Folders / Playlists / Consoles + carátula
  abajo) │ tabla │ barra de transporte inferior. Reusa los mismos modelos
  Rust que las pestañas de mini: `refresh_library` ya es el único punto que
  decide qué se pinta — la shell desktop es otra vista de los mismos datos,
  no otro estado.
- La tabla del spike 1 se convierte en el componente real.
- Interacción de escritorio que mini no tiene: hover, click derecho (el
  `ContextMenu` genérico), doble click, rueda, `Ctrl+A`/flechas/`Enter`/
  `Delete`, tooltips.

**Hito de cierre**: con la ventana ancha se ve la shell de tres paneles con
la biblioteca real y suena música; con la ventana estrecha, mini exacto.

## Fase 3 — Paridad de features, por orden de dolor

Cada bloque es entregable por separado; el desktop Tauri no se apaga hasta
que el último cierra.

**3a. Reproducción** (casi todo llega hecho de la fase 1): selector de
dispositivo en Ajustes, panel DSP/EQ (el formulario de `DspSettings.svelte`
en Slint contra `tunante-audio`), loop count vgm, filtro de pistas cortas,
continue-from-queue. Ratings/favoritos en la UI (la BD ya los tiene; mini
nunca los pintó) con la prioridad configurable BD/tag/`_ratings.m3u`.

**3b. Biblioteca**: watcher conectado a la nueva app, orden por columna
persistido, sort con collation (el `Intl.Collator` de `sort.ts` se sustituye
por colación Unicode en Rust), drop de carpetas del SO → playlist, resync,
fast scan, progreso de escaneo en el sidebar.

**3c. Herramientas** — los tres diálogos grandes del desktop, en Slint:
- Editor de metadatos (989 ln Svelte): edición por lote, info técnica,
  reclasificación embebida.
- Reclasificación (855 + 84 ln): consola + juego con sugerencias
  (Libretro/biblioteca/Steam), ámbito carpeta/pista, worklist de sin
  clasificar.
- Carátulas: picker manual con rejilla de candidatas y campo de búsqueda,
  descarga masiva con preview/cancel/**undo por corrida**, 5 modos de encaje.
Todo el trabajo de red/lógica ya está en `tunante-art` y los comandos
extraídos en fase 1; esto es pintar formularios.

**3d. Integración de escritorio**: tray completo (spike 2 industrializado:
menú, tooltip con pista, 3 estilos de icono, scroll-volumen — el popup GTK
nativo se sustituye por una ventanita Slint sin marco, que además unifica
Linux/macOS/Windows donde hoy hay dos implementaciones), close-to-tray,
single instance (fichero de lock + socket/D-Bus para «enfoca la ventana»),
atajos globales + botones de ratón, updater (portable casi entero; pierde
`relaunch()` de Tauri → exec de sí mismo), entrada `.desktop`, ventana de
debug, **tema claro** (el `Theme` global de mini ya existe; hay que poblar la
paleta light y conmutarla — y seguir `prefers-color-scheme` vía el crate
`dark-light` o el portal de D-Bus en Linux), título de ventana con la pista,
sesión con auto-resume <5 min.

Lo que la app nueva gana gratis respecto al desktop Tauri: **MPRIS** (mini ya
lo trae; el desktop nunca lo tuvo), inhibición de suspensión, y el boost de
scheduler para el teléfono.

## Fase 4 — El vuelco

1. `apps/mini` → `apps/desktop` (o `apps/tunante`); el directorio Tauri +
   SvelteKit **se borra entero**: `src-tauri`, `src`, `package.json`,
   `node_modules`, adapter-static, Tailwind. No queda npm en el repo.
2. La BD: lógica de adopción/importación de la sección de arriba, ejecutada
   una vez al arrancar.
3. CI: fuera los jobs de node y de `tauri build`; los tarballs glibc
   x86_64/aarch64 y el .apk Alpine que hoy salen de mini pasan a ser *las*
   releases de escritorio. `scripts/gen-icons.py` pierde los 30 iconos de
   `src-tauri/icons/` y gana los del bundle nuevo. El updater apunta a los
   assets nuevos (cuidado con la transición: la última release Tauri debe
   saber actualizar hacia el formato nuevo, o se documenta el salto manual).
4. Empaquetado que Tauri regalaba y hay que reponer: AppImage/deb para Linux
   (linuxdeploy o cargo-bundle), .app para macOS, instalador Windows — solo
   los que se publiquen hoy realmente.
5. Android no se toca: su UI es Compose por decisión previa
   (`plan-android.md`) y consume los mismos crates.

## Fase 5 — Cosecha

- Auditar qué quedó sin segundo consumidor y devolverlo a la app; qué ganó
  segundo consumidor (¿Android quiere el watcher?) y bajarlo al crate.
- Medir y presumir: binario sin WebKit ni node_modules, arranque, RAM con la
  biblioteca de 29.500 cargada, tiempo de compilación sin `tunante-codec` en
  la app.
- Borrar de `CLAUDE.md` y de los docs todo lo que hablaba de dos apps.

## Riesgos, con nombre

| Riesgo | Tamaño | Mitigación |
|---|---|---|
| La tabla de 17 columnas en Slint no rinde o exige pelearse con `ListView` | **El grande** | Spike 0.1 antes de nada; plan B: columnas fijas + anchos configurables (menos flexible que el desktop, quizá aceptable) |
| Tray SNI sin bucle GTK anfitrión | Medio | Spike 0.2; hilo GTK dedicado es patrón conocido; peor caso: tray solo con menú, sin scroll-volumen |
| DnD externo no expuesto por Slint | Pequeño | Escape hatch winit; peor caso: botón «añadir carpeta» (ya existe) |
| macOS/Windows: mini nunca ha corrido ahí; App Nap, CGEventTap, fd-limit, popup, updater .app son código escrito para Tauri+AppKit | Medio | Slint/winit corre en ambos; portar los workarounds uno a uno en 3d; decidir *pronto* si macOS/Windows son objetivo del primer release del stack nuevo o llegan después |
| El refactor de `commands/` (2.900 ln) introduce regresiones silenciosas | Medio | Fase 1 lo hace **contra la app Tauri viva**, que actúa de arnés de pruebas de la lógica extraída |
| Paridad larga: dos desktops conviviendo meses | Cierto y asumido | Por eso el orden es «mini crece»: en todo momento hay una app completa que funciona (la vieja) y una que mejora (la nueva); nunca un estado intermedio roto |

## Lo que este plan deliberadamente no hace

- No traduce los 79 comandos ni los 8 stores: esa capa existe porque había
  dos procesos y dos lenguajes. En la app final son llamadas de función y
  structs.
- No unifica la UI de Android: decisión ya tomada en su plan.
- No toca `vendor/` ni los crates compartidos salvo para *recibir* mudanzas.
- No promete fecha. El orden de fases sí promete una cosa: en ningún punto
  intermedio el usuario se queda sin reproductor completo.
