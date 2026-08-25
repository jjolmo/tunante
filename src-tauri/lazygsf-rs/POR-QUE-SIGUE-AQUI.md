# Por qué este directorio sigue existiendo

`lazygsf-rs` **ya no se compila**. No es dependencia de nadie desde que
`tunante-codec` pasó a leer las etiquetas GSF con `viogsf-rs`, que es el crate
que además reproduce ese formato.

Eran 73 MB de mGBA vendorizado para **una sola función**, `read_gsf_tags`.

La sustitución se comprobó contra música real antes de hacerla: 276 ficheros
`.minigsf`, de los cuales **3 difieren, y en 1 milisegundo** de duración
(152175 frente a 152174) — redondeo al interpretar el formato `M:SS.mmm` de la
etiqueta, no un desacuerdo real.

## Entonces, ¿por qué no se borra?

Porque `viogsf-rs/CMakeLists.txt` compila **el `psflib` que vive aquí**:

    set(PSFLIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../lazygsf-rs/psflib")

Es decir: de este directorio sólo hace falta `psflib/`. Todo lo demás —el core
de mGBA, que es el grueso— es peso muerto que ya no entra en ninguna
compilación, pero que tampoco se puede quitar sin mover `psflib` primero y
tocar la construcción de viogsf, que es el camino de reproducción de GSF.

**Si alguien quiere terminar la poda:** mover `psflib/` a `src-tauri/psflib/`,
apuntar ahí `viogsf-rs/CMakeLists.txt`, y entonces sí borrar el resto. El
renombrado de símbolos (`-D psf_load=viogsf_psf_load`) tiene que seguir intacto:
cinco crates enlazan su propia copia de psflib en el mismo binario, y ese
renombrado es lo único que evita la colisión.
