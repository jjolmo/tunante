# Paridad fina: lo que el desktop Tauri (v0.1.283) hacía y el nuevo aún no

> 2026-09-02. Origen: el usuario, probando la app con las manos, encontró en
> minutos lo que la lista de paridad gorda no medía (doble click roto, columnas
> inmóviles, el racimo del transporte descolocado). Este documento sale de leer
> el código fuente COMPLETO del desktop borrado (`577901c~1`: 21 componentes,
> stores, 85 comandos, cada handler de teclado y ratón) y cruzarlo contra el
> mini actual. Es la lista de trabajo. La lista gorda de plan-desktop-slint.md
> medía capacidades; esta mide comportamiento.

## A · Roto o grave (bloquea el uso diario)

- [x] **Doble click no reproducía** — cada click repintaba la selección con
      `set_vec`, matando el TouchArea entre click y click. Arreglado 3e6b5a8.
- [ ] **No se puede quitar una carpeta de la biblioteca.** El viejo tenía en
      Ajustes → Library la lista de carpetas monitorizadas con ✕ por carpeta
      (y toggle Watch individual). Mini solo sabe añadir. Grave: una carpeta
      equivocada es para siempre.
- [ ] **Reordenar columnas arrastrando la cabecera** (umbral 5px, la arrastrada
      a opacity .5, borde accent en el destino; mismo gesto sin umbral = click
      para ordenar). Pedido explícitamente por el usuario.
- [ ] **Redimensionar columnas** (handle de 5px, mínimo 40px, anchos
      persistidos en `column_config` junto a visibilidad y orden).
- [ ] **El knob de loops para formatos streamed no viaja**: `probe_opts` pasa
      `vgm_loop_count: None`, así que la clave del viejo (0–20 loops para
      BRSTM/ADX/HCA…) ni se lee ni se expone en Ajustes. Las duraciones de esos
      formatos no obedecen al usuario.
- [ ] **Errores de reproducción invisibles**: el viejo tenía ErrorToast
      ("Failed to play \"X\": …", autodescarte 8 s). Mini los manda a stderr;
      en pantalla una pista rota simplemente "no suena".

## B · Falta funcional clara

**Tabla**
- [ ] 5 columnas del catálogo viejo: Album Artist, Disc #, Sample Rate,
      Channels, y la combinada **Album/Game** con su ajuste de preferencia.
- [ ] Click central en fila = encolar/desencolar.
- [ ] Número de posición en cola en la columna de estado (cuando no suena).
- [ ] Ctrl+Shift+click: rango AÑADIDO a la selección existente.
- [ ] Tooltips por celda con el valor completo (todo elidido hoy es ilegible).
- [ ] Menú contextual de la cabecera (click derecho = toggles de columnas sin
      cerrar el menú; hoy solo existe el ⚙).
- [ ] «Quitar de la cola» / «Quitar de la lista» contextuales según contexto.
- [ ] Persistir orden de ordenación entre sesiones (columna + dirección).
- [ ] Scroll automático a la pista sonando al restaurar la ventana.

**Sidebar**
- [ ] Carpetas monitorizadas (raíces) listadas con contador — hoy solo pineadas.
- [ ] Doble click reproduce (All Tracks, playlist, carpeta, consola) —
      aleatoria si shuffle.
- [ ] Botón «+» nueva playlist en el sidebar (input inline, Enter/Escape).
- [ ] Menú contextual de playlist en sidebar: Enqueue all / Rename / Delete
      (hoy viven solo en la vista Listas).
- [ ] Menú contextual de carpeta pineada: «Abrir en gestor de archivos».
- [ ] Ancho del sidebar redimensionable (150–500px, persistido).
- [ ] Barra de progreso de escaneo al pie del sidebar.
- [ ] Reordenar playlists arrastrando.

**Árbol / archivos**
- [ ] Buscador de carpetas («Find folder...», resultados planos, máx 50).
- [ ] Contadores de pistas por carpeta propagados hacia arriba.
- [ ] Persistir carpetas expandidas.
- [ ] Menú contextual de carpeta: Play / Crear playlist / Reclasificar /
      Pin / Abrir en gestor (hoy parcial).
- [ ] Compactación de cadenas de un solo hijo (A/B/C estilo VS Code).

**Transporte**
- [ ] Click en el bloque now-playing = navegación inteligente a la pista
      (vista actual → su consola → su carpeta → All Tracks) con scroll.
- [ ] Botón mute (volumen 0 ↔ 0.8) con icono según nivel.
- [ ] Toggle de fade/crossfade en la barra (hoy enterrado en Ajustes).
- [ ] Título de ventana dinámico «Título - Artista — Tunante» (+ su toggle).

**Atajos y teclado**
- [ ] Pestaña Shortcuts entera: 11 acciones reconfigurables (badge de tecla,
      grabar pulsación, botón de ratón por acción con modificador, ámbito
      global/app, Reset all). Mini hoy: 3 atajos fijos por portal + botones
      de ratón fijos.
- [ ] Ctrl+P abre Ajustes; atajo para enfocar la búsqueda.
- [ ] Teclas peladas configurables in-app (volume up/down, mute, shuffle,
      repeat, fav…).

**Ajustes que faltan como filas**
- [ ] Toggle auto-descarga de carátulas (hoy siempre activa al reproducir).
- [ ] «Store covers in folder».
- [ ] Acción del click central del tray (5 opciones en el viejo).
- [ ] Toggles de secciones del sidebar (Appearance → show faved/playlists/
      consoles/files/folders/cover).
- [ ] Watch on/off por carpeta.
- [ ] Loops de formatos streamed (ver A).

**Arranque / sesión**
- [ ] Reconciliación de ratings al arrancar (el viejo pasaba disco→BD en
      segundo plano: 29.530 en ~1,5 s; sin ella un `_ratings.m3u` editado
      fuera no se refleja hasta re-escanear).
- [ ] Persistir texto de búsqueda.
- [ ] Auto-reanudación condicionada a <5 min desde el cierre (mini reanuda
      siempre en pausa; el viejo solo si cerró sonando hace poco).

## C · Menor / pulido

- [ ] Spinner en el panel de carátula mientras se busca.
- [ ] Estados vacíos con mensaje («No tracks in library» + pista de qué hacer).
- [ ] Registro: filtro por nivel, campo de filtro, Copy, Clear, auto-scroll
      (el viejo DebugWindow tenía todo eso).
- [ ] Popup de feedback de volumen al hacer rueda sobre el tray.
- [ ] Diálogo de crash (panic hook → crash.log + zenity/kdialog).
- [ ] «Fix the lengths too» y ámbito «solo esta pista» en nombres de pistas.
- [ ] Type-ahead de consola rankeado por codec (escribir `spc` → SNES).
- [ ] About: versión visible, créditos, enlace al repo.

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
