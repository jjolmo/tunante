# Paridad fina: lo que el desktop Tauri (v0.1.283) hacía y el nuevo aún no

> 2026-09-02. Origen: el usuario, probando la app con las manos, encontró en
> minutos lo que la lista de paridad gorda no medía (doble click roto, columnas
> inmóviles, el racimo del transporte descolocado). Este documento sale de leer
> el código fuente COMPLETO del desktop borrado (`577901c~1`: 21 componentes,
> stores, 85 comandos, cada handler de teclado y ratón) y cruzarlo contra el
> mini actual. Es la lista de trabajo. La lista gorda de plan-desktop-slint.md
> medía capacidades; esta mide comportamiento.
>
> **CERRADA (2026-09-02).** Todo migrado. Los dos únicos «[~]» que quedan no
> son huecos sino decisiones de diseño escritas: «Quitar de la lista» vive
> donde las listas se ven (la tabla es SIEMPRE la biblioteca, no una lista),
> y la auto-reanudación restaura pista+posición en pausa a propósito (una app
> que arranca sonando sola es hostil). Nada del inventario del viejo (79
> comandos, 21 componentes) se quedó sin equivalente.

## A · Roto o grave (bloquea el uso diario)

- [x] **Doble click no reproducía** — cada click repintaba la selección con
      `set_vec`, matando el TouchArea entre click y click. Arreglado 3e6b5a8.
- [x] **Quitar carpetas de la biblioteca** — lista de raíces en Ajustes con
      ✕ por carpeta y toggle vigilada/sin vigilar. La poda respeta a las
      supervivientes (`remove_tracks_by_folder_path_excluding`: una carpeta
      anidada absorbida no se lleva la música de otra raíz), y sync_watches
      ahora reconcilia en ambos sentidos (para de vigilar lo quitado).
- [x] **Reordenar columnas arrastrando la cabecera** — el gesto del viejo:
      umbral de 5px separa click-para-ordenar de arrastre, la arrastrada baja
      a opacity .5, el destino se marca con borde accent, las celdas viajan
      con su cabecera, y el orden guardado ES el orden de pintado (adiós al
      corsé de catálogo; una columna re-activada llega al final).
- [x] **Redimensionar columnas** — handle de 5px con cursor col-resize y
      brillo accent, mínimo 40px, negocia el ancho con la vecina derecha
      (el par conserva su suma: nada se desborda), matemática siempre desde
      el snapshot del press (sin deriva), pesos persistidos como
      `key:peso` dentro de mini.table_columns.
- [x] **Loops de formatos streamed** — fila «Bucles de formatos streamed»
      (predet./1/2/3/5/10) sobre la clave `vgm_loop_count` del desktop; la
      leen AMBOS lados (probe_opts para el escáner, engine para la
      reproducción — antes ninguno), así barra y oídos cuentan lo mismo.
- [x] **Errores de reproducción visibles** — toast en la esquina («No se
      pudo reproducir: …»), click lo cierra, el timer lo envejece a los 8 s
      (el reloj del viejo). Cablea los tres caminos de play del usuario;
      stderr sigue recibiendo copia.

## B · Falta funcional clara

**Tabla**
- [x] 4 columnas nuevas: Artista del álbum, Disco, Muestreo (Hz), Canales
      (mono/estéreo/N) — con celda y ordenación. Y la combinada
      **Álbum / Juego** con su fila «La columna Álbum/Juego enseña»
      (álbum/juego, clave album_game_prefers del desktop).
- [x] Ordenación persistida entre sesiones (claves session_sort_column/
      _direction del desktop).
- [x] Click central en fila = encolar/desencolar de la cola de usuario.
- [x] Ctrl+Shift+click: el rango se SUMA a la selección existente.
- [x] Posición en cola visible en la fila: «»N» en accent delante del título
      mientras la pista espera en la cola de usuario (huella barata en el
      timer; solo repinta filas cuyo badge cambió).
- [x] «Quitar de la cola» en el menú contextual (mismo toggle que el click
      central).
- [x] Tooltip de fila con retardo de SO (700 ms): título completo, artista ·
      juego · álbum y la ruta — todo lo que la elipsis esconde.
- [x] Menú contextual de la cabecera: click derecho lista columnas con ✓
      (el multi-tick sin cerrar del viejo lo conserva el ⚙).
- [~] «Quitar de la lista»: existe donde las listas viven (la hoja de la
      vista Listas); la tabla no muestra listas, así que ahí no significa
      nada. «Quitar de la cola» ya está en su menú.
- [x] Scroll a la pista al restaurar: en el único evento de «volver» que
      este stack ve — reaparecer desde la bandeja — la tabla aterriza en lo
      que suena (reutiliza el salto de now-clicked). Los demás restores no
      emiten evento observable en Slint/winit.

**Sidebar**
- [x] Carpetas monitorizadas (raíces) en el sidebar con contador, delante de
      las pineadas — click acota la tabla al subárbol, doble click reproduce;
      sin ✕ (las raíces se quitan en Ajustes, donde la salida poda).
- [x] Doble click reproduce en el sidebar — Pistas, playlists, carpetas
      pineadas Y consolas; arranque aleatorio si shuffle.
- [x] «+» junto a Listas (salta a la vista donde nacen; el input inline del
      viejo queda como pulido).
- [x] Menú contextual de playlist en sidebar: Reproducir / Encolar todo /
      Renombrar… (abre su vista) / Borrar.
- [x] Menú contextual de carpeta en sidebar: Reproducir / Abrir en el gestor
      de archivos / Quitar de aquí (solo pineadas).
- [x] Ancho del sidebar redimensionable — la costura de 4px es el asa
      (cursor ew-resize, se ilumina en accent), 150–500px, persistido en
      mini.sidebar_width.
- [x] Progreso de escaneo al pie del sidebar — y con arreglo de fondo: en
      desktop un re-escaneo YA NO sustituye la app entera por la pantalla de
      progreso; la UI sigue usable y el pie informa (el teléfono conserva su
      pantalla: no tiene esquina que ceder).
- [x] Reordenar playlists arrastrando en el sidebar — umbral de media
      fila, la arrastrada al 50%, borde accent en el destino, y el click
      posterior al arrastre queda suprimido (el gesto no abre la lista).

**Árbol / archivos**
- [x] Buscador de carpetas — la búsqueda del árbol antepone hasta 50
      carpetas cuyo nombre casa; tocar una la REVELA en el árbol
      (expandiendo ancestros) y limpia la búsqueda.
- [x] Contadores por carpeta: ya eran de subárbol (contents().total cuenta
      descendientes) — estaba hecho y la lista no lo sabía.
- [x] Carpetas expandidas persistidas (files_expanded_folders, la clave
      del viejo), guardadas en cada toggle.
- [x] Menú de carpeta completo: además de encolar/añadir-a-lista, ahora
      Reproducir carpeta, Reclasificar…, Fijar en el sidebar y Abrir en el
      gestor de archivos.
- [x] Compactación de cadenas de un solo hijo (a/b/c, estilo VS Code) —
      carpetas sin pistas y con un único subdirectorio colapsan en una fila;
      la clave de expansión es el final de la cadena.

**Transporte**
- [x] Click en el bloque now-playing = saltar a la pista: selecciona,
      cursor y scroll-into-view; si un filtro/Favoritos/carpeta la tapa,
      ensancha a la biblioteca entera y repite (los peldaños consola→carpeta
      del viejo quedan sin replicar — el ensanche cubre el caso).
- [x] Botón mute (0 ↔ último volumen, 0.8 si nadie recuerda) con glifo por
      nivel 🔇/🔉/🔊.
- [x] Toggle de crossfade en la barra (∵∴, recuerda la última duración).
- [x] Título de ventana dinámico «Título - Artista — Tunante», con su
      toggle «Título en la barra de la ventana» (show_track_in_titlebar).

**Atajos y teclado**
- [x] 11 acciones con tecla grabable en Ajustes (click en la fila → la
      siguiente pulsación queda; Escape cancela, Supr desvincula). Claves
      `shortcut.<id>`; despacho por FocusScope a nivel de shell con
      burbujeo desde la tabla, y los inputs de texto conservan sus teclas.
      El viejo tampoco traía defaults: todo se graba. Quedan de su pestaña:
      botón de ratón POR ACCIÓN con modificador (los botones del pulgar son
      fijos next/prev vía evdev) y el ámbito global por tecla (lo global va
      por los 3 atajos del portal).
- [x] Ctrl+P abre Ajustes (fijo, como el viejo); enfocar búsqueda es la
      acción `focus_search`, grabable.
- [x] Teclas peladas configurables in-app (volumen ±, mute, shuffle,
      repeat, favorito…).

**Ajustes que faltan como filas**
- [x] «Descarga automática de carátulas» (off por defecto, como el viejo) —
      al sonar una pista sin arte lanza UNA búsqueda por el resolver del
      bulk (confianza High), un intento por pista y sesión. Corrección de
      diagnóstico: mini NO descargaba nada al reproducir — el hueco era la
      función entera, no el toggle.
- [x] «Guardar carátula en la carpeta» (store_covers_in_folder).
- [x] Acción del click del tray, configurable (5 opciones del viejo:
      mostrar/ocultar, reproducir/pausa, stop, siguiente, siguiente con
      fundido — este último fuerza el fade aunque esté apagado, volteando
      el flag del engine solo para ese cambio). Medido antes: ayatana
      enruta Activate y SecondaryActivate al MISMO target, así que en este
      stack hay un canal de click y la acción viaja en él.
- [x] Toggles de secciones del sidebar: carátula, Favoritos, Carpetas y
      Listas, bajo las claves show_* del desktop. (Consolas/árbol no son
      secciones del sidebar nuevo.)
- [x] Watch on/off por carpeta (entró con la lista de raíces del bloque A).
- [x] Loops de formatos streamed (ver A).

**Arranque / sesión**
- [x] Reconciliación de ratings al arrancar — por el pipe, no in-process
      (la regla del decoder): subcomando `resolve-ratings` que recibe
      `rating\tpath` por stdin y devuelve solo los que el disco desmiente,
      con el mismo orden de prioridad; mini aplica el diff en un hilo y
      library_dirty repinta. Medido en vivo: 29.530 en 0,48 s (el viejo,
      1,5 s in-process).
- [x] Texto de búsqueda persistido (search_query), flush por el timer.
- [~] Auto-reanudación <5 min: cubierto distinto a propósito — mini
      restaura pista y posición SIEMPRE pero en pausa; nunca suelta sonido
      solo por abrirse, que es lo que la regla de 5 min acotaba.

## C · Menor / pulido

- [x] Sección Consolas en el sidebar — siluetas (ConsoleArt) y contadores,
      click acota, doble click reproduce, menú Reproducir/Carátulas.
- [x] Filas de solo lectura del editor de metadatos (duración · códec ·
      muestreo · canales · bitrate · tamaño · ★), una línea bajo la ruta.
- [x] «Descargar automáticamente» en la hoja de carátula (refetch: olvida
      la caché, coge la mejor del archivo, la aplica sobre lo que haya).
- [x] ETA humanizada y nombre del elemento en el progreso del bulk de
      carátulas («~3 min», «menos de un minuto»).

- [x] «buscando carátula…» bajo el sidebar mientras la auto-descarga está
      en vuelo (cover-busy, apagado por art_dirty).
- [x] Estados vacíos de la tabla: «Nada casa con la búsqueda» vs «No hay
      pistas — añade carpetas desde Ajustes».
- [x] Registro con toolbar: nivel (todo/aviso+/solo error), filtro de texto,
      Copiar (wl-copy→xclip) y Limpiar; el refresco del timer respeta ambos
      filtros.
- [x] Feedback de volumen del tray: la rueda mueve el slider Y el tooltip
      del icono muestra «Volumen N%» ~1,5 s (el buzón del tooltip cede el
      turno). Sin ventana flotante aparte — el tooltip ES el popup.
- [x] Diálogo de crash: panic hook → tunante-crash.log en XDG data +
      zenity/kdialog con el mensaje y la ruta.
- [x] «Arreglar también las duraciones» en la hoja de nombres: off deja el
      campo de duración vacío y el re-sellado por probe pone la real (solo
      cambian los títulos). El ámbito «solo esta pista» del viejo no aplica:
      mini re-lee el fichero entero por probe, no una pista suelta.
- [x] Type-ahead de consola por códec: caja de filtro sobre el desplegable
      que rankea nombre exacto > prefijo > códec propio (spc → SNES) >
      substring, con «(automática)» siempre arriba.
- [x] «Acerca de Tunante»: fila con versión y autor; click abre el repo.

## D · Cubierto distinto (no es deuda)

Búsqueda con plegado de acentos (mejor que el viejo) · cola como panel con
reorder por arrastre · bulk covers con preview+undo (el viejo tenía el flujo
"preview" SIN construir) · fix de nombres con más salvaguardas · updater con
skip-version · rating de lo-que-suena con 5 estrellas (el viejo: 0↔5 de la
seleccionada) · registro in-app · vigilancia de carpetas con test.

## E · Deliberadamente no (motivo escrito)

Drop de carpetas del SO (Wayland/winit no lo entrega) · drag de pistas a
playlists como GESTO (la intención va por menú; el arrastre visual con badge
«♫ N tracks» queda aspiracional) · macOS · ventana volume-popup nativa.

---

El orden de ataque razonable: **A entero**, luego B por bloques (tabla →
sidebar → atajos → ajustes → sesión), C al final. D y E no son trabajo.
