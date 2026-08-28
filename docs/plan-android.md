# Tunante en Android — plan de proyecto futuro

> Estado: **Fases 0, 1, 2, 3, 5 y 6 terminadas y verificadas en hardware el
> 2026-08-27.** La 4, la interfaz, tiene su primera versión funcionando en el
> móvil: falta rellenarla, no decidirla.
>
> A día de hoy la app abre su base de datos, escanea una carpeta real del
> almacenamiento compartido, y reproduce de ahí con el decodificador en su
> propio proceso, el sonido saliendo por AAudio, y **la cola avanzando sola con
> la app en segundo plano** desde un servicio en primer plano con su
> MediaSession. Lo único que le falta para ser un reproductor es la interfaz.
>
> Escrito el mismo día midiendo el acoplamiento del código que hoy corre en
> postmarketOS, tras dos decisiones del proyecto: **la interfaz será nativa de
> Android**, no un port de la de Slint, y **el proyecto vive en `android/`**
> dentro de este mismo repositorio.
>
> **Aparato de pruebas:** Samsung Galaxy S23 (`SM-S911B`), Android 16, API 36,
> arm64-v8a, páginas de 4 KB. Vía `adb`, con el binario en `/data/local/tmp`.
> **El emulador también, y ahora se usa**: el APK lleva `arm64-v8a` y `x86_64`,
> así que la interfaz se puede mirar sin depender de un teléfono que puede estar
> bloqueado — que es exactamente lo que pasó, y por lo que la rejilla estuvo un
> rato sin poder verse. Sigue **sin sustituir al móvil para los cores**: en
> x86_64 `lazyusf2` activa `ARCH_MIN_SSE2` y el resampler de DeSmuME entra por
> su rama SSE, o sea que compila y ejecuta C distinto. Vale para los píxeles,
> no vale para «¿siguen funcionando los decodificadores?».
>
> `ABIS="arm64-v8a" ./build.sh` se salta la segunda compilación cuando sólo
> importa el teléfono.
>
> **NDK en uso:** r27.3.13750724, el que ya estaba instalado. Ver Fase 6 para
> por qué habrá que subir a r28+ antes de empaquetar.
>
> En español porque su hermano `plan-postmarketos.md` lo está, y son el mismo
> género de documento.

## Veredicto

**No es una locura. Es un mes**, y con la interfaz nativa deja de tener riesgos
serios: pasa a ser un mes de trabajo previsible.

Las tres cosas que podían matarlo de raíz —los cores de C, el decodificador
fuera de proceso y las rutas absolutas de la biblioteca— están las tres bien.
Lo caro no es lo exótico; lo caro es escribir el caparazón Android que la app
no tiene todavía. Y con la interfaz nativa, ese caparazón deja de ser trabajo
*contra* el framework y pasa a ser trabajo *con* él.

Tres sorpresas buenas, comprobadas y no supuestas:

1. **Los cores emuladores ya están portados y nadie se dio cuenta.** El trabajo
   de hacerlos compilar en aarch64/musl/clang para el móvil es exactamente el
   mismo que hace falta para bionic/NDK. No hay JIT compilado (el dynarec de
   lazyusf2 está fuera a propósito), no hay autotools, no hay SIMD de x86 activo,
   no hay cabeceras exclusivas de glibc en la ruta compilada. Quedan **tres
   líneas de enlazado y un tamaño de pila**.

2. **El decodificador fuera de proceso sobrevive.** Esta era la que parecía
   condenada, porque «Android no deja lanzar binarios». La regla real es más
   estrecha: SELinux prohíbe `execve` sobre el *directorio de datos* de la app,
   pero **sí lo permite sobre `nativeLibraryDir`**. Un binario empaquetado como
   `libtunante_decoder.so` con `extractNativeLibs="true"` se ejecuta. Es lo que
   hace Termux, en Google Play. De `decoder.rs` cambia **una función**:
   `decoder_path()`. El protocolo —cabecera JSON y luego f32 crudo por una
   tubería— no se toca.

3. **La biblioteca no hay que reescribirla, porque no vas a publicar en Play.**
   El almacenamiento con ámbito rompería `walkdir`, las rutas absolutas en
   SQLite y las consultas `LIKE 'carpeta/%'`, y rompería peor de lo que parece
   (ver «Lo que casi nos come»). Pero `MANAGE_EXTERNAL_STORAGE` devuelve POSIX
   entero, y ya instalas por `wget` desde una release de GitHub. La política que
   lo prohíbe es de Play, no del sistema.

---

## La decisión que cambia el plan: interfaz nativa

Este es el cambio de mayor efecto de todo el documento, y conviene entender por
qué antes de leer las fases.

**Lo que se evita.** En Android, Slint sólo tiene Skia: `renderer-femtovg` está
marcado `cfg(not(target_os = "android"))` y el renderizador por software está
compilado fuera. O sea, la escotilla documentada `SLINT_BACKEND=winit-software`
—que existe justo porque no te fías del Adreno 618— **no tiene equivalente en
Android**. Ese era el riesgo número uno del plan anterior, y desaparece entero.
Con él se van las ~34 incidencias abiertas del IME de Slint en Android, el
`Key.Back` roto en Android 14+, la pelea con los recortes de pantalla, y la
dependencia de fontconfig.

**Lo que cuesta.** Reescribir 2938 líneas de `app.slint` en Compose. Es trabajo
de bulto, pero sin incógnitas: `app.slint` es la especificación y `theme.slint`
son los tokens de color y tamaño ya decididos —48 px de objetivo táctil, la
paleta al estilo foobar2000—, así que «se tiene que ver igual» es transcribir,
no diseñar.

**Lo que se gana además.** El caparazón Android —servicio en primer plano,
MediaSession, foco de audio— era la mitad del presupuesto del plan anterior, y
lo era porque había que escribirlo en Kotlin y pegarlo por JNI a una app que era
un `android_main` de Slint. Con una app Android normal, eso deja de ser pegamento
y pasa a ser el camino de en medio del framework. **La parte cara se abarata al
mismo tiempo que aparece la parte nueva.**

### Dónde va la frontera Rust / Kotlin

Rust se queda con **todo lo que hay por debajo de los píxeles**:

- `tunante-core`: la base de datos, la cola, `vgm_path`. Sin tocar.
- `tunante-codec` y el proceso `tunante-decoder`: sin tocar.
- El escaneo de biblioteca, la reproducción, el estado del reproductor.

Kotlin se queda con **la app Android**: Activity, Compose, el servicio en primer
plano, MediaSession, el foco de audio, los permisos.

La alternativa —Room para la base de datos, ExoPlayer con un `Renderer` a medida
alimentado desde Rust— tira por la borda `tunante-core` entero, que es
justamente lo que este proyecto ha invertido en tener reutilizable. Descartada.

**La superficie JNI, en JSON.** Un puñado de llamadas sobre un objeto
`TunanteEngine`, devolviendo JSON: `open(dbPath)`, `scan(roots)`,
`tracksInFolder(path)`, `play(path)`, `pause()`, `seek(ms)`, `next()`,
`state()`. No es elegante y es lo correcto: `serde_json` ya está en el árbol,
`tunante-decoder` ya habla JSON, y evita marshalling de objetos JNI a mano, que
es donde se van las tardes. Unos miles de filas de JSON no son un problema en un
teléfono de 2020, y la interfaz pagina igualmente.

**Un efecto colateral que ya queríamos.** Hoy *todo* lo que depende del tiempo
cuelga de un único `slint::Timer` de 500 ms: avanzar la cola al acabar la pista,
drenar los mandos, el temporizador de apagado, guardar la sesión, actualizar la
posición. Sacar ese reloj de la interfaz era una tarea pendiente del plan
anterior; en esta arquitectura **no hay dónde ponerlo salvo en el servicio**.
Sale gratis, por construcción.

Y con él se arregla otra cosa: la sesión se guarda hoy cada 5 segundos *desde el
temporizador de la interfaz*, y no al salir. Con un enganche en `onPause`, el
comentario de `session.rs:36` —«a un móvil lo mata el sistema más veces de las
que lo cierra el usuario»— por fin se cumple de verdad.

---

## Dónde vive el código

**En `android/`, dentro de este repositorio. No un repositorio anidado, no un
repositorio aparte.**

El motivo es una dependencia, no una preferencia: el proyecto Android necesita
`tunante-core`, `tunante-codec` y `tunante-decoder` como dependencias por ruta.

- **Repositorio aparte** obliga a meter `tunante` como submódulo. Pero `tunante`
  arrastra el suyo propio (`vgmstream`) y un buen montón de C vendorizado, así
  que sería un submódulo recursivo pesado — y cada arreglo en un core exigiría
  actualizar el puntero del submódulo antes de que Android lo viera. Se paga una
  fricción diaria por una limpieza que sólo se nota al clonar.
- **Repositorio anidado dentro de éste** es lo peor de ambos: git lo trata como
  directorio sin seguimiento o como gitlink no declarado, según el día, y tienes
  el dolor del submódulo sin su contabilidad.
- **`android/` en este repositorio** hace que las dependencias por ruta
  funcionen solas, y hace que un cambio en `tunante-codec` que rompa Android se
  vea en el mismo push que lo causó. A Gradle le da igual dónde vive.

El precedente ya existe: `mini.yml` filtra por rutas, así que un flujo de
Android encaja igual, sin que cada push arrastre al otro.

**El único coste real** es que `release.yml` sube una etiqueta en cada push, así
que la app Android hereda la numeración de tunante. Discutible sólo en teoría:
es tunante, y `tunante-mini` ya vive con ello.

**Trampa de nombres que hay que resolver antes de la primera release:** los
paquetes de Alpine y los de Android se llaman los dos `.apk`. Hoy ya se cuelga
`tunante-mini-0.1.261-r0.apk` de la etiqueta. El de Android tiene que llamarse
distinto de forma evidente — `tunante-android-0.1.261.apk` — o alguien acabará
instalando el que no era.

---

## Lo que casi nos come, y conviene tener escrito

Si algún día se quiere Google Play, esto deja de ser un plan y pasa a ser una
reescritura. Bajo `READ_MEDIA_AUDIO` —el permiso que Play sí permite— el acceso
por ruta existe pero pasa por FUSE, y MediaProvider decide fichero a fichero con
una condición SQL: la fila tiene que existir en MediaStore y su `media_type`
tiene que ser audio. Y `media_type` se deduce de **la extensión**, contra el
mapa de MIME de Android.

`.vgz`, `.psf`, `.psf2`, `.minipsf`, `.nsf`, `.spc`, `.gsf`, `.2sf`, `.ssf`,
`.usf` no están en ese mapa. No se indexan, luego no se pueden abrir, luego
`readdir()` **ni siquiera los lista**. Un reproductor de música normal no nota
nada; este exactamente no funciona.

Y el modo de fallo es cruel: `stat()` funciona, `access(F_OK)` devuelve 0, y
`open()` da EACCES. Un escáner ingenuo ve metadatos plausibles y luego se
estrella al leer.

La salida sin `MANAGE_EXTERNAL_STORAGE` sería SAF: descriptores de fichero en
vez de rutas, y URIs de documento en vez de rutas en la base de datos. Eso es
tocar `tunante-codec` entero, porque los cores de C hacen `fopen()` por dentro.
**Es la reescritura que este plan evita al elegir la vía lateral.**

**Comprobado en el aparato el 2026-08-27, ya no es inferencia.** Un `.psf` y un
`.nsf` en almacenamiento compartido salen de MediaProvider como
`application/octet-stream` con `media_type=0`, y un `.flac` a su lado como
`audio/flac` con `media_type=2`. Ver Fase 3.

---

## Decisiones ya tomadas

1. **Interfaz nativa** (Compose), no Slint. Ver arriba.
2. **`android/` en este repositorio.** Ver arriba.
3. **Distribución por sideload / F-Droid.** No es una preferencia, es lo que
   sostiene la decisión de almacenamiento. Queda escrito para que dentro de un
   año nadie lo reabra sin ver el precio.
4. **El decodificador sigue fuera de proceso.** La tentación de meterlo en el
   hilo es fuerte y es un error: matar el proceso es cómo se recuperan los
   ~43 MB de RAM de consola emulada, y es lo que aísla las globales de C entre
   pistas. En un móvil con la memoria contada eso vale más, no menos.
5. **`minSdk` 26.** Es el suelo de cpal (AAudio). No hay margen por abajo.

---

## Plan por fases

Cada fase responde **una pregunta que puede matar el proyecto**, en el orden en
que más barato sale enterarse. Si una puerta no pasa, se para ahí.

### Fase 0 — ¿Compilan los cores y suenan? ✅ **HECHA**

Sin interfaz, sin Gradle, sin APK. Sólo `tunante-codec` cruzado a
`aarch64-linux-android` y su prueba de humo de formatos corriendo en el móvil de
verdad, empujada con `adb push` a `/data/local/tmp` — el dominio `shell` sí
puede ejecutar desde ahí, así que esto se prueba sin empaquetar nada.

**Resultado, en el S23, `--release`, 1,91 s:**

```
psf  psf2  gsf  2sf  usf  gme/nsf  vgmstream  opus  wav  flac  ogg  mp3  m4a
```

trece formatos, todos con PCM que no es silencio, más la cadena DSP y las cuatro
variantes de resolución de bibliotecas GSF. `8 passed; 0 failed`. El escritorio
sigue igual de verde, con el mismo listón.

Que **`psf` y `psf2` pasen es la prueba del arreglo de la pila**: sexypsf corre
su intérprete de PSX justo en ese hilo.

El binario sale PIE aarch64 con intérprete `/system/bin/linker64` y
`libc++_shared.so` entre sus `NEEDED`, que es donde se comprueba de verdad el
cambio del runtime de C++ — **un `rlib` no enlaza**, así que `cargo build` a
secas no habría probado nada.

Cómo repetirlo:

```sh
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.3.13750724
cargo ndk -t arm64-v8a --platform 26 test -p tunante-codec --release --no-run
# empujar binario + fixtures + libc++_shared.so a /data/local/tmp/tunante, y:
adb shell 'cd /data/local/tmp/tunante && \
  LD_LIBRARY_PATH=. TUNANTE_FIXTURES_DIR=./fixtures ./smoke --nocapture'
```

`TUNANTE_FIXTURES_DIR` es nuevo, y hacía falta: `fixtures_dir()` usaba
`env!("CARGO_MANIFEST_DIR")`, una ruta absoluta **del anfitrión horneada en el
binario**, que en un móvil no existe. Sin ese escape, ningún test cruzado puede
encontrar sus datos.

Arreglos aplicados, todos pequeños:

- `vgmstream-rs/build.rs:58` — `target.contains("linux")` **acierta** con
  `aarch64-linux-android`, así que emite `-lstdc++`. Bionic tiene un
  `libstdc++.so` que es un muñón: sólo `operator new`/`delete`. Hay que emitir
  `-lc++_shared`.
- `vio2sf-rs/build.rs:98` y `viogsf-rs/build.rs:41` — igual, pero peor: usan
  `#[cfg(target_os = "linux")]` dentro de un build script, que es el cfg del
  **anfitrión**, no del objetivo. Ya estaban mal; nunca había importado.
  Comparar cadenas de `TARGET`.
- `hepsf-rs/sexypsf_wrapper.c:77` — `pthread_create(t, NULL, …)`, pila por
  defecto. El `-z stack-size=8388608` que arregla esto en musl **no sirve en
  Android**: bionic no lee `PT_GNU_STACK`, fija ~1 MiB y punto. Hace falta un
  `pthread_attr_setstacksize` explícito.
- `viogsf-rs` y `vgmstream-rs` usan el crate `cmake` y necesitan
  `CMAKE_TOOLCHAIN_FILE`, `ANDROID_ABI` y `ANDROID_PLATFORM` **como
  `.define()`**, no como variables de entorno: `cmake-rs` mira sus propios
  defines para decidir si está en modo NDK y no lee el entorno que exporta
  `cargo-ndk`. Es un fallo conocido y el parche lleva sin fusionarse desde
  diciembre de 2025.
- `vgmstream` lanza `sh version-make.sh`, que llama a `git describe` y escribe
  dentro del submódulo. La imagen de compilación necesita `sh` y `git`, y el
  árbol tiene que ser escribible.

**Puerta:** todos los backends abren un fixture real y devuelven PCM que no es
silencio, en un Android aarch64. Es exactamente el listón que ya usa la CI.

### Fase 1 — ¿Sale sonido desde una app de verdad? ✅ **HECHA**

Una app Android mínima —una Activity con botones, en Java, desechable— que carga
`libtunante_android.so`, lanza el helper y reproduce.

**Resultado, en el S23:**

```
 8526  1688  com.tunante.android
 8563  8526  libtunante_decoder.so      <- hijo del proceso de la app
```

y en `dumpsys audio`:

```
piid:5543 type:AAudio state:started deviceIds:[3] usage=USAGE_MEDIA sampleRate=44100
```

O sea, las dos cosas que esta fase existía para responder: **el `execve` desde
`nativeLibraryDir` funciona** con `targetSdk 34`, es decir bajo el régimen
estricto de W^X, y **cpal llega a AAudio** y el flujo sale enrutado a un
dispositivo real.

Lo que hay montado: `crates/tunante-android` (cdylib JNI) y `android/` (proyecto
Gradle, AGP 8.5.2, Gradle 8.7, Java, `minSdk` 26 / `targetSdk` 34). Se construye
con `android/build.sh`, que hace el cargo-ndk, coloca las `.so` en `jniLibs` y
llama a Gradle. Los fixtures viajan dentro del APK como assets, así que esta
fase no debe nada al almacenamiento — eso es la Fase 3.

Se dispara sin tocar la pantalla, que es como se probó con el móvil bloqueado y
como lo hará la CI:

```sh
adb shell am start -S -n com.tunante.android/.MainActivity --es play sample.psf
```

El `-S` no es adorno: sin él, `am start` sobre una actividad viva entrega
`onNewIntent` y **`onCreate` no vuelve a correr**, así que la prueba parece
pasar sin haber ejecutado nada.

#### Las dos cosas que costaron, y por qué

**`LD_LIBRARY_PATH` para el hijo.** El decodificador moría antes de `main()` con
`CANNOT LINK EXECUTABLE: library "libc++_shared.so" not found`, teniendo esa
biblioteca **en el mismo directorio**. Las bibliotecas propias de la app cargan
porque ART construye un espacio de nombres del enlazador y le pasa
`nativeLibraryDir`; un proceso arrancado con `execve` no hereda nada de eso,
sólo el entorno — y el entorno de la app no nombra ese directorio. Se arregla
con un `.env("LD_LIBRARY_PATH", …)` en el `Command`.

**El `stderr` a `/dev/null`.** `tunante-mini` lo manda ahí y en un escritorio es
razonable, porque acaba en la terminal. En Android el `stderr` de un hijo no va
a ninguna parte, así que un decodificador que muere al arrancar es **totalmente
mudo e indistinguible de una pista que se acabó**. El diagnóstico de arriba
costó tres intentos a ciegas y apareció en el primer log en cuanto se drenó el
`stderr` a logcat, en un hilo (sin hilo, la tubería se llena a los 64 KB y
bloquea al decodificador a mitad de pista).

#### Lo que queda pendiente de esta fase

`useLegacyPackaging = true` es obligatorio y no es una optimización de tamaño
que se pueda apagar: AGP lo pone en `false` desde `minSdk` 23, lo que deja las
bibliotecas mapeadas desde el APK sin desempaquetar — bien para algo que se
`dlopen`, fatal para algo que se ejecuta, porque no hay fichero que darle a
`execve`.

Sigue pendiente **la duplicidad de cpal** (0.16 por `game-music-emu`, 0.17 por
rodio). No ha dado guerra todavía; sigue en la lista de riesgos.

- **`ndk_context` hay que inicializarlo a mano.** En el plan anterior lo hacía
  `android-activity` por nosotros; sin Slint, no hay quien lo haga. cpal lo
  necesita para enumerar dispositivos y **entra en pánico si falta**. Va en
  `JNI_OnLoad`: `ndk_context::initialize_android_context(vm, activity)`. Ojo,
  inicializarlo dos veces dispara un `assert`.
- `decoder_path()` pasa de `current_exe().parent()/tunante-decoder` a
  `ApplicationInfo.nativeLibraryDir/libtunante_decoder.so`, pasado desde Kotlin.
  El helper se compila contra el NDK, **no contra musl**: el hijo hereda el
  filtro seccomp del zygote y un binario de glibc estático se muere durante la
  inicialización de libc.
- `extractNativeLibs="true"` es obligatorio; AGP lo pone en `false` por defecto
  desde `minSdk` 23. Afecta a los dos `.so`, no sólo al helper.
- cpal ya no usa Oboe: desde 0.16 el backend de Android es **AAudio por
  `ndk::audio`**. `player.rs` —que sólo llama a `open_default_sink()` y
  `connect_new()`— no debería tocarse.
- Ojo con esto: rodio 0.22.2 fija cpal 0.17.1, y el árbol arrastra además cpal
  0.16.0 por `game-music-emu`. **Dos backends de AAudio y dos consumidores de
  `ndk_context` compilados a la vez.** Confirmar que la dependencia de gme es
  sólo de desarrollo antes de que muerda.
- `output.rs` entero sale con `cfg`: `pactl` no existe aquí.

**Bifurcación a decidir aquí, no antes:** si AAudio pelea con el foco de audio o
con el enrutado, la alternativa es que Kotlin sea el dueño de un `AudioTrack` y
Rust le sirva PCM. Es más idiomático en Android pero es trabajo; empezar con
rodio/cpal, que es cero trabajo y ya está validado en este código.

**Puerta:** suena una pista por el altavoz, incluido un `.psf`, con el
decodificador en su propio proceso.

### Fase 2 — El puente JNI ✅ **HECHA**

La superficie descrita arriba, y el escaneo de biblioteca funcionando desde
Java. Sin interfaz todavía: se valida con logs y una lista sin estilo.

#### El coste del escaneo, medido: no es un problema

Era el riesgo número uno del plan tras caer los otros dos. Medido en el S23, 200
ficheros, un proceso por fichero:

| | total | por fichero |
|---|---|---|
| 200 × `/system/bin/true` (suelo del `exec`) | 918 ms | 4 ms |
| 200 × `tunante-decoder probe` | 849 ms | 4 ms |
| 200 × `probe --fast` | 682 ms | 3 ms |

**El probe cuesta lo mismo que ejecutar un binario que no hace nada.** O sea que
el coste es enteramente el arranque del proceso, no la lectura de metadatos, y
el arranque son 4 ms. Una biblioteca de 2000 pistas es ~8 s en un solo hilo, y
el escaneo ya usa `available_parallelism() - 2`.

Para comparar: el temporizador de `probe` en `tunante-mini` es de **20 segundos**
por fichero. Hay cinco mil veces de margen.

Queda por confirmar dentro de la app, que es otro dominio SELinux y lleva el
filtro seccomp del zygote; esto se midió desde `adb shell`. Pero con ese margen,
la conclusión no está en duda: **el modo por lotes en el helper no hace falta**.

#### Hecho: un solo cliente del decodificador

`tunante-mini/src/decoder.rs` pasa a ser el crate `tunante-helper`, del que
dependen mini y android. Lo dije al escribir la copia de la Fase 1 y era la
deuda que tocaba pagar antes de que las dos versiones divergieran.

Movido con `git mv`, así que conserva su historia. Los dos arreglos que salieron
en Android van dentro y benefician a ambos:

- `LD_LIBRARY_PATH` para el hijo, bajo `#[cfg(target_os = "android")]` —
  deliberadamente no en escritorio, donde anteponer un directorio a la ruta de
  búsqueda podría ensombrecer una biblioteca del sistema sin motivo.
- El `stderr` del decodificador drenado a `log` en un hilo, en vez de a
  `/dev/null`.

La ruta del helper es ahora inyectable (`set_decoder_path`), porque mini la
deduce como hermana del ejecutable y Android no puede: allí `current_exe()` es
`/system/bin/app_process64`.

Los dos tests del resampler viajaron con el fichero y siguen pasando.

#### Y medido **dentro** de la app, que es lo que faltaba

Lo de arriba se midió desde `adb shell`, que es otro dominio SELinux y no lleva
el filtro seccomp del zygote. Dentro de la app, con `nativeScan`:

| ficheros | total | por fichero |
|---|---|---|
| 200 | 2 267 ms | 11,3 ms |
| 800 | 9 082 ms | 11,4 ms |

Perfectamente lineal, y **casi el triple que desde el shell** pese a ir en
paralelo con seis hilos. La explicación más probable es el coste de `fork` desde
un proceso grande: la app arrastra el montón de ART y copiar sus tablas de
páginas no es gratis, mientras que en el shell se bifurca un proceso diminuto.

No cambia la conclusión —2000 pistas son ~23 s, contra un temporizador de 20 s
**por fichero**— pero sí cambia el número que hay que citar. Por eso la
medición del shell no bastaba.

#### La superficie

`nativeOpenDb(dir)`, `nativeScan(root)`, `nativeTracks(folder)`, más las de
reproducción. Todas devuelven JSON, incluidos los errores, como
`{"ok":false,"error":…}`. **Nada lanza excepciones a través de JNI**: una
excepción pendiente sin comprobar convierte la *siguiente* llamada JNI en un
`abort`, y una sola forma de respuesta es mucho más difícil de estropear que
acordarse de comprobar.

La base de datos vive en `Context.getFilesDir()`, que no está detrás de FUSE y
admite WAL sin discusión.

### Fase 3 — Almacenamiento ✅ **HECHA**

**Y de paso quedó zanjada la duda que este documento marcaba como inferencia.**
Empujados tres ficheros a `/storage/emulated/0/Music/tunante-test/` y
preguntado a MediaProvider en un Android 16:

```
sample.psf   mime_type=application/octet-stream   media_type=0   <- no es audio
sample.nsf   mime_type=application/octet-stream   media_type=0   <- no es audio
sine.flac    mime_type=audio/flac                 media_type=2   <- sí lo es
```

Exactamente lo que decía «Lo que casi nos come», ahora medido y no supuesto:
bajo `READ_MEDIA_AUDIO` —el permiso que Play permite— la condición es
`media_type IN (2,4,5)`, así que **el `.psf` y el `.nsf` fallan la comprobación
y son ilegibles**, mientras que el `.flac` de al lado se abre sin problema. Con
`.flac` la app parece funcionar; con la biblioteca de verdad, no.

Los tres estados, medidos:

| permiso | resultado del escaneo |
|---|---|
| ninguno | **0 ficheros** — el directorio no se puede ni listar |
| `MANAGE_EXTERNAL_STORAGE` | los 3, `.psf` y `.nsf` incluidos |

Y la carpeta `Music` real del teléfono: 16 pistas de 22 ficheros, a 11,1 ms cada
uno.

Cierra el círculo lo último que se probó: **reproducir el `.psf` desde
`/storage/emulated/0/…`**, o sea un core en C haciendo `fopen()` sobre una ruta
FUSE, en su propio proceso, sonando por AAudio. Que era la única duda que
quedaba sobre si el diseño entero se sostenía en Android.

`picker.rs` no se ha portado: en Android el selector de carpetas será el nativo,
y eso va con la interfaz (Fase 4).

#### Lo que decía el plan antes de medirlo

`MANAGE_EXTERNAL_STORAGE` pedido con
`Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION`, comprobado con
`Environment.isExternalStorageManager()`. Con eso, `walkdir`, `read_dir`,
`is_dir`, las rutas absolutas en SQLite y las consultas por carpeta siguen
funcionando **tal cual**.

`picker.rs` deja de ser Rust: el selector de carpetas pasa a ser el nativo de
Android, arrancando en `/storage/emulated/0` y sabiendo de la tarjeta SD. Hoy
arranca en `~/Musica` y puede subir hasta `/`.

**Puerta:** escanear `/storage/emulated/0/Music` encuentra los `.vgz` y los
`.psf`. Esta es también la prueba que zanja la duda del mapa de MIME.

### Fase 4 — La interfaz 🟡 **primera versión en pie**

Compose con **Kotlin 2.0.21, AGP 8.5.2** y la BOM de Compose 2024.09.03. Lo que
hay hoy, funcionando en el S23:

- **`ui/Theme.kt`**, los trece tokens de `theme.slint` transcritos, con la misma
  regla que allí: ningún color literal fuera de ese fichero. Y el mismo
  `mmss()`, con las horas plegadas en los minutos para que la fila no cambie de
  ancho al cambiar de pista.
- **Lista de biblioteca** con la fila en curso resaltada en `bg-selected`,
  duraciones a la derecha, y respaldo al nombre de fichero cuando no hay
  etiqueta de título.
- **Barra de reproducción** con progreso, transporte y `posición / duración`.
- **Banner de permiso**, en ámbar. No es adorno: sin acceso a todos los ficheros
  el escaneo no encuentra literalmente nada, así que sin él la primera
  ejecución es una biblioteca vacía sin explicación.
- Objetivos táctiles de 48 dp, incluido uno que se estira con su texto — el
  cuadrado partía «Escanear» en dos líneas y se leía como dos botones.

- **Árbol de carpetas** con miga de pan, derivado de las rutas de las pistas y
  no de `read_dir`: así se sigue navegando aunque la tarjeta no esté puesta.
- **Búsqueda**, que *reemplaza* el árbol en vez de filtrarlo — cuando buscas un
  título no te importa en qué carpeta estaba, y un árbol filtrado te obliga a
  bajar para averiguarlo. Es lo que hace mini.
- **Botón atrás** enganchado a la misma acción que la miga.

Un primitivo nuevo en el puente por un motivo concreto: `nativePlayList`, que
encola una lista explícita. `nativePlayFolder` no servía porque
`get_tracks_by_folder` casa `path LIKE 'carpeta/%'` e incluye las subcarpetas,
así que sus índices no cuadran con una lista que muestra sólo lo que hay
directamente en la carpeta — y habrías puesto a sonar una pista distinta de la
que tocaste.

- **Carátulas**, en la fila y en la barra de reproducción. Dos fuentes en orden:
  la incrustada en las etiquetas del fichero, que cuesta un proceso, y luego un
  `cover.jpg` al lado, que cuesta un `read_dir`. La mayoría de los rips de
  consola tienen lo segundo y no lo primero. `folder_image` se movió a
  `tunante-helper::art` para que mini y android den la misma respuesta.

  La caché se limita **por bytes y no por número de entradas**: una portada de
  1600×1600 y una miniatura de 200×200 son «una entrada» cada una y se
  diferencian en sesenta veces la memoria. El hueco se reserva siempre, haya
  carátula o no — una lista cuyas filas cambian de alto según van llegando las
  imágenes es peor que una sin carátulas.

- **Poda**. Antes una carpeta borrada del disco seguía en el árbol, porque el
  árbol sale de lo escaneado. `prune_missing` se ejecuta antes de cada escaneo.
  Va **aparte de `scan_folder` a propósito**: escanear una carpeta de una tarjeta
  que resulta no estar montada borraría media biblioteca en silencio si podara
  a ciegas.

- **Listas de reproducción**, en su propia pestaña: crear, abrir, borrar con
  confirmación de dos toques —no hay deshacer detrás, y una lista es lo único
  aquí que hizo el usuario— y añadir pistas con pulsación larga sobre la fila.

  La pulsación larga y no un botón en cada fila: un botón competiría con la fila
  por el dedo, y la fila es lo que se pulsa noventa y nueve veces de cada cien.

  El puente añade por **ruta y no por id**, porque la ruta es lo que tiene la
  pantalla: el id de una pista es un UUID que nadie ve nunca. Las rutas que la
  biblioteca no conoce se saltan en vez de inventarlas — una entrada de lista
  que apunta a nada es peor que una que falta.

- **Pestañas** Biblioteca / Listas, y el botón atrás sale de la lista abierta,
  luego de la pestaña, luego de la carpeta.

- **Retomar dónde lo dejaste.** `session.rs` se movió de `tunante-mini` a
  `tunante-core` —no depende más que de `Database`— y ahora lo usan los dos. Se
  guarda cada diez ticks del servicio, o sea cada cinco segundos, **y además en
  `onPause`**: ese enganche es el que el plan pedía y el que la cadencia de
  cinco segundos no puede dar, porque salta justo cuando el sistema está más
  cerca de matar el proceso.

  Probado con un `force-stop` en seco, que es como muere de verdad una app en un
  móvil: `resumed …/dxlegends_intro.ogg at 4856 ms, paused`. **Vuelve en pausa a
  propósito** — un teléfono que se pone a sonar en un bolsillo porque se
  reinició es peor que uno que se olvidó.

  Las claves del `settings` siguen diciendo `mini.` aunque el módulo ya no sea
  de mini: renombrarlas perdería en silencio la posición guardada de quien ya lo
  usa, a cambio de una cadena más limpia. Cada app tiene su propio fichero de
  base de datos de todas formas.

- **Aleatorio, repetición y temporizador de apagado**, en una segunda fila de la
  barra. Segunda fila y no más glifos junto al transporte: son ajustes que se
  tocan poco y el transporte es lo que busca el pulgar, así que no comparten su
  sitio. El temporizador cuenta contra ticks de reloj de pared y no contra
  muestras reproducidas, que es lo que quiere decir «en veinte minutos».

  `SleepTimer` no tenía pruebas ni en mini; ahora tiene cinco, y cubren lo que
  de verdad duele: que **cero minutos signifique cancelar y no silenciar ahora
  mismo** —están a un toque de distancia en la interfaz— y que **dispare una
  sola vez**, porque uno que dispara dos pausa una pista que acabas de reanudar.

Y quedó visto por fin en pantalla algo de la Fase 5 que hasta ahora sólo estaba
en `dumpsys`: **los controles en la pantalla de bloqueo**, con el título y el
transporte de la MediaSession.

- **Deslizar para actuar**, el gesto que mini pone en sus filas: deslizar una
  pista de la biblioteca la **encola**, deslizar una dentro de una lista la
  **quita**. Cualquiera de los dos sentidos hace lo mismo — preguntar hacia
  dónde deslizaste es un examen, no una interfaz. Umbral en un tercio de la
  fila: lejos para que no lo dispare un pulgar despistado, cerca para no exigir
  un barrido entero de una pantalla de seis pulgadas.

  El gesto se **reclama** en vez de compartirse: la fila vive dentro de una
  lista que se desplaza en vertical, y `detectHorizontalDragGestures` sólo gana
  cuando el movimiento es más horizontal que vertical. Un manotazo diagonal
  mientras recorres una lista larga debe desplazar, no encolar en silencio.

- **Rejilla de carpetas con carátula**, 3 columnas en vertical y 8 en apaisado
  — los mismos números a los que reteja mini. No son una proporción: un móvil
  tumbado es mucho más ancho que alto, y una rejilla que sólo doblara dejaría
  las teselas del tamaño de la miniatura de una miniatura.

  Sale sólo en los niveles que son **únicamente carpetas**, que son una
  estantería y merecen verse como portadas. Un nivel con pistas dentro es una
  lista de pistas, y ahí las carátulas empujarían los títulos fuera de la
  pantalla.

Y un arreglo que delató el propio log del sistema: el servicio empujaba el
estado a MediaSession **en cada tick, aunque estuviera en pausa y no hubiera
cambiado nada**, lo que hacía que `MediaSessionService` registrase y
retransmitiese un cambio de estado dos veces por segundo para siempre. Ahora
sólo cuando se mueve.

#### Tres cosas que salieron al releer lo escrito deprisa

- **Retomar podía sonar.** `restore` cargaba la pista llamando a `play()` y
  hacía `pause()` justo después. Microsegundos, pero la fuente ya está
  encolada y el mezclador puede tirar de un búfer en ese hueco — o sea un
  chasquido en la única cosa que esta app promete no hacer al arrancar.
  `start()` recibe ahora un `autoplay`, y la ruta de restaurar no desactiva la
  pausa en ningún momento.

- **La rejilla podía poner decenas de emuladores en marcha a la vez.**
  `nativeArtwork` lanza un proceso decodificador y espera hasta cinco segundos,
  y `Dispatchers.IO` corre sesenta y cuatro corrutinas sin despeinarse: agitar
  una rejilla de ocho columnas era eso multiplicado, cada uno con la RAM de su
  consola dentro. Ahora hay un semáforo de tres.

- **El árbol se construía en el hilo de la interfaz.** `nativeBrowse` lee la
  tabla de pistas entera para derivar un nivel; en una colección de verdad eso
  es una lectura completa más su JSON, y no es algo que se haga entre dos
  fotogramas. Fuera del hilo principal.

Y una más que delató el log del sistema y se midió antes y después: el servicio
empujaba el estado a MediaSession **en cada tick, cambiara o no**, y el sistema
lo registraba y retransmitía. Doce cambios cada seis segundos en pausa; ahora,
cero.

#### Y verificado por fin en pantalla

La rejilla y el retejado no se habían podido ver: el móvil se quedó bloqueado en
su pantalla de PIN. Añadir `x86_64` al APK lo resolvió de raíz — 3 columnas en
vertical y 6 carátulas en una fila en apaisado, sobre una biblioteca de prueba
de seis carpetas con portada propia.

De paso quedó descartado un falso positivo: las carátulas parecían no salir, y
lo que pasaba es que la primera captura era **anterior a que resolvieran**. Se
piden en segundo plano y de tres en tres. No había nada que arreglar, y merece
quedar escrito para que nadie «arregle» eso.

#### El árbol tenía un fallo de verdad, y ahora tiene pruebas

En la raíz mostraba **todas las carpetas que contienen pistas, planas**. Una
biblioteca anidada como `Music/Rock/Beatles/Abbey Road` abría en `Abbey Road` en
vez de en `Rock`, y dos carpetas llamadas `Disc 1` de álbumes distintos salían
idénticas y sin forma de distinguirlas, porque lo único que muestra una fila es
el último componente.

La derivación se movió a `tunante-core::tree`, que es lógica pura sobre cadenas
y por tanto la única parte de la pantalla de biblioteca que se puede probar sin
un teléfono enchufado. Las raíces son ahora **un nivel por debajo de lo que
todas las pistas tienen en común**.

Ocho pruebas, y la que más costó pensar: el ancestro común se calcula
**componente a componente, no byte a byte**. `/m/Sonic` y `/m/Sonic 2` comparten
el prefijo de bytes `/m/Sonic`, que además es un directorio real — y con eso
`Sonic 2` habría desaparecido dentro de `Sonic`.

#### Una comprobación nueva en la CI, por un susto propio

Reescribiendo eso borré por accidente catorce funciones JNI de un tirón —listas,
carátula, sesión, temporizador— y **todo siguió compilando**. Java declara sus
`native` y Rust exporta sus símbolos, y entre los dos lados no hay ningún
sistema de tipos: lo que sale de ahí es un `UnsatisfiedLinkError` la primera vez
que un dedo toca la pantalla que lo llama.

`android.yml` compara ahora las dos listas y falla si Java declara algo que Rust
no exporta. Es la única costura de este diseño sin comprobación estática, así
que le toca una prueba.

#### Y una pérdida de datos que llevaba ahí desde antes de Android

Yendo a poner pruebas a `prune_missing` —la única función de todo esto que
**borra pistas**— salió que el fallo no estaba en ella sino debajo. Las cuatro
consultas de `tunante-core` que buscan por prefijo de ruta construían un patrón
`LIKE` a partir de la ruta **sin escaparla**, y en SQLite `_` significa
«cualquier carácter» y `%` significa «lo que sea».

Los nombres de carpeta están llenos de guiones bajos; esta misma biblioteca
tiene un `sky_temple-the-ark`. La prueba que lo demostró:

```
borrar /m/sky_temple  ->  se llevó también /m/skyXtemple
left: []   right: ["/m/skyXtemple/b.mp3"]
```

**Dos de esas consultas son `DELETE`, y las usa el vigilante de carpetas del
escritorio**, así que esto no era un problema de Android: estaba esperando a que
alguien tuviera una carpeta con guión bajo y otra parecida al lado. Todas van
ahora por un `like_prefix` que escapa `%`, `_` y la propia barra, con
`ESCAPE '\'`. Incluidas las de «conservar», donde el fallo va en la dirección
mala: un prefijo que no casa es una carpeta borrada que debía salvarse.

`prune_missing` tiene cinco pruebas, y la que importa es que **podar una carpeta
no toca otra**: una tarjeta SD desenchufada es una carpeta llena de ficheros que
parecen todos ausentes, y podar la biblioteca entera por eso no tiene vuelta
atrás.

#### El reloj del reproductor, y por qué acabó en el core

Revisando `Player::seek` de Android salió esto:

```rust
self.accumulated = Duration::from_millis(ms);
self.started_at = Some(Instant::now());   // <- siempre
```

Ponía el reloj en marcha **aunque la pista estuviera en pausa**, así que arrastrar
la barra sin reproducir dejaba la posición subiendo sola — en la pantalla y en lo
que la MediaSession le cuenta al sistema. Tampoco clampaba a la duración ni
comprobaba que el `try_seek` hubiera funcionado.

`tunante-mini` lo tenía bien las tres veces. O sea que no era un problema
difícil: era una reimplementación de veinte líneas hecha de memoria en vez de
compartida, exactamente lo que `CLAUDE.md` dice que no se haga.

Ahora es `tunante_core::clock::PlayClock`, con ocho pruebas, y lo usan los dos.
La que da nombre al módulo: **buscar no es reanudar** — un reloj en pausa al que
le mueves la posición sigue en pausa.

#### Siguiendo la pista: cuatro divergencias más con mini

Dicho al acabar la ronda anterior: donde quedan fallos es en la lógica
reescrita de memoria en vez de compartida. Comparando `Player` con el de mini
línea a línea salieron cuatro, y una es gorda.

**La duración salía de la base de datos y no del decodificador.** El escaneo
pregunta con `probe --fast`, que informa de la longitud que declara el fichero;
`play` recibe `--loops 2 --fade 8000` y produce un flujo tan largo como eso lo
haga. Para el mismo `sample.psf`:

```
probe --fast                     duration_ms = 16000
play --loops 2 --fade 8000       duration_ms = 31000
```

Casi el doble. **La música de consola hace bucle por diseño**, o sea que esto es
casi toda la biblioteca: la barra se llenaba a mitad de la pista y se quedaba
clavada al 100 % el resto, y la MediaSession publicaba una duración equivocada.
mini siempre la ha tomado de `source.total_duration()`, que es la cabecera que
el decodificador acaba de devolver. Verificado en el móvil: `now playing [0/1]
sample.psf (31000 ms)`.

Las otras tres, más pequeñas:

- **El temporizador de apagado pausaba en vez de parar.** Pausar deja el proceso
  decodificador vivo toda la noche con la RAM de su consola dentro; matarlo es
  cómo vuelve esa memoria. mini para.
- **`stop()` no reseteaba la duración**, así que la sesión de medios seguía
  publicando la de lo que sonaba antes.
- **`player.clear()` donde mini usa `player.stop()`.**

Lo que sí coincidía, y me alegró comprobarlo: el temporizador de apagado sólo
cuenta mientras sale sonido, con el mismo razonamiento que mini escribió en su
día — «uno que expira durante una pausa es una promesa rota en la dirección
mala».

#### Selector de carpetas y escaneo multi-raíz

Hasta ahora el escaneo estaba clavado en `/storage/emulated/0/Music`. Si tu
música vive en otro sitio, no había forma de decírselo.

Ahora hay un selector —un explorador de directorios sobre rutas reales, como
`picker.rs` de mini, y **no** `ACTION_OPEN_DOCUMENT_TREE`: el selector de
documentos devuelve un `content://` sin ruta detrás, y todo lo que hay debajo
(`walkdir`, la base de datos, los cores en C que hacen `fopen` por nombre)
trabaja con rutas. Con el acceso a todos los ficheros ya concedido, pasar por
ahí no compra nada.

Las carpetas elegidas van a `monitored_folders`, que el core ya tenía, y un
escaneo sin argumento las recorre todas. Verificado en el emulador con música en
dos sitios: `scanned 11 files ... across 2 roots`.

«Escanear» **reescanea** con un toque y abre el selector con una pulsación
larga. La primera versión abría el selector siempre, lo que hacía pasar la
acción frecuente por la pantalla de la acción rara: elegir dónde vive la música
se hace una vez, reescanear se hace muchas. Sin raíces todavía, el toque corto
abre el selector igualmente — un escaneo sin raíces no tiene nada que decir.

Dos decisiones más:

- **Quitar una raíz olvida sus pistas.** Dejarlas pondría filas en la biblioteca
  que ningún escaneo va a refrescar ni podar nunca — peor que perderlas, porque
  no habría forma de quitarlas.
- **La poda va por raíz, nunca entre raíces.** Una tarjeta sin montar es una
  raíz llena de ficheros que parecen ausentes; podar la biblioteca entera por
  eso no tendría vuelta atrás.

#### Una pista imposible envenenaba cada arranque

Sólo salió al instalar en el móvil de verdad, con música de verdad. La sesión
guardada apuntaba al `Over_the_Horizon.m4a` de Samsung — el Dolby que lofty
acepta y symphonia no puede abrir. Restaurar fallaba, el error se propagaba, y
la app arrancaba así **todas las veces, para siempre**, porque nada llegaba a
sobrescribir la posición guardada. Encima dejaba la cola con una pista que no
podía sonar.

Ahora una pista que no abre no tumba el arranque: se registra el motivo, se
vacía la cola y se sigue. Una pista guardada puede dejar de ser reproducible
entre una ejecución y la siguiente —borrada, en una tarjeta que no está puesta,
o de un formato que el decodificador no maneja— y ninguna de esas cosas debería
impedir abrir la app.

### Fase 7 — lo que la interfaz todavía no tenía

Salió de preguntarse en serio «¿está todo?» en vez de darlo por hecho. Cuatro
cosas, en orden de lo que pesa:

**1. Bucles y desvanecido.** Estaban fijos en 2 y 8 s. En un reproductor de
música de consola eso no es un ajuste menor: casi todo el repertorio hace bucle
por diseño y no tiene final propio, así que esos dos números *son* la duración
de la pista. mini los cicla `1 → 2 → 3 → ∞` y `0 → 4 → 8 → 15 s` y los guarda en
`settings`; las mismas claves y los mismos pasos, para que las dos apps se
comporten igual y una biblioteca compartida no cambie de sonido al cambiar de
programa.

**2. Listas: renombrar, reordenar, encolar entera.** El core ya tenía
`rename_playlist`, `reorder_playlists` y `reorder_playlist_tracks` sin usar.

**3. Una vista de cola de verdad.** Hoy hay cuenta y «vaciar». Falta quitar una
suelta y reordenar. `PlayQueue` sabe encolar y desencolar por id pero no mover,
así que eso hay que añadirlo al core — con pruebas, porque mover elementos de
una lista por índice es donde se cuelan los off-by-one.

**4. Crear una lista desde una pista**, sin ir antes a la pestaña de listas.

Nada de esto necesita puente nuevo salvo llamadas: la frontera Rust/Kotlin ya
está donde tiene que estar.

**Lo que falta**: nada de la lista original. Queda pulir, no construir.

Dos cosas que en Slint eran problema y aquí salieron gratis, como decía el plan:
el **botón atrás** y los **recortes de pantalla**.

### Fase 4 — lo que decía el plan (1,5-2 semanas)

Transcribir `app.slint` a Compose. `theme.slint` da la paleta y los tamaños;
`app.slint` da la estructura, los gestos y los casos raros ya resueltos: 38
`TouchArea`s, deslizar para actuar coordinado contra el `Flickable`, el
retejado a 3 columnas en vertical y 8 en horizontal, la miga de pan `◂`.

Dos cosas que en Slint eran problema y aquí son gratis: el **botón atrás**
—`HANDOVER.md:74` dice «no hay botón atrás» porque Plasma Mobile no lo tiene—
y los **recortes de pantalla**, barra de estado, muesca y teclado. En Compose
son API de primera clase.

Es la fase más larga y la que menos puede salir mal.

### Fase 5 — El caparazón de reproducción ✅ **HECHA**

`PlaybackService`, servicio en primer plano de tipo `mediaPlayback`, verificado
en el S23:

```
isForeground=true  foregroundId=1  types=0x00000002   (= FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
audio focus: granted
```

Y la prueba que importa — con la app mandada a segundo plano con `HOME`:

```
16:55:12  now playing [6/12] Play the Organ! Part 2      <- un PSF de ~31 s
16:55:43  now playing [7/12] 10-sample                   <- avanzó sola
```

31 segundos después, sin nadie mirando, con AAudio en `state:started`. **El
reloj vive en el servicio**, que era el cambio estructural que el plan pedía:
en `tunante-mini` ese temporizador es un `slint::Timer` en el hilo de la
interfaz, y por eso allí todo lo temporal se para cuando se para la ventana.

Lo que hay montado:

- **MediaSession** del framework con `PlaybackState` y `MediaMetadata`, más
  notificación `Notification.MediaStyle`. Da controles en la pantalla de bloqueo
  y hace funcionar los botones de un casco Bluetooth.
- **Foco de audio** con `AudioFocusRequest`, pausando en `AUDIOFOCUS_LOSS` y
  reanudando en `GAIN`, y dejando que el sistema agache el volumen en el caso
  `CAN_DUCK`.
- **`ACTION_AUDIO_BECOMING_NOISY`**, que pausa al desconectar auriculares —
  seguir sonando por el altavoz es lo más grosero que puede hacer un
  reproductor.
- **Wake lock parcial**, el equivalente directo del inhibidor de logind que mini
  toma en postmarketOS.

**Sin AndroidX.** Todo con APIs del framework: `android.media.session.MediaSession`
y `Notification.MediaStyle`. Añadir `androidx.media3` para conseguir una
notificación con tres botones no compensa, y esta app no tiene ninguna
dependencia de AndroidX.

Una nota de método: el tick sólo reconstruye metadatos y notificación **cuando
cambia la pista**, no dos veces por segundo. A 500 ms, rehacerlos siempre es
mucha basura para una pantalla que nadie está mirando.

#### Lo que decía el plan

- **Servicio en primer plano de tipo `mediaPlayback`.** No es opcional ni son
  buenas maneras: desde Android 17, una app sin actividad visible **necesita**
  uno para reproducir en segundo plano, y el fallo es **silencioso** —
  `requestAudioFocus()` devuelve `AUDIOFOCUS_REQUEST_FAILED` y no salta ninguna
  excepción.
- **MediaSession** + notificación `MediaStyle`. Sustituye a `mpris.rs`. Como
  aquello, da los controles en la pantalla de bloqueo y los botones del casco
  —que en este teléfono siguen siendo la superficie de control principal, porque
  el jack de 3,5 mm no tiene driver.
- **Foco de audio**, con pausa al recibir llamada y agachado de volumen, y el
  receptor de `ACTION_AUDIO_BECOMING_NOISY`. cpal no hace **nada** de esto.
- **Wake lock parcial** en lugar del inhibidor de logind.
- **El reloj de 500 ms vive aquí**, no en la interfaz. Ver arriba.

### Fase 6 — Empaquetado y CI ✅ **HECHA**

`.github/workflows/android.yml`, y salió más simple que `mini.yml` justo por lo
que el plan predijo: aquí se cruza de verdad, así que corre en un runner x86_64
normal, **sin runner aarch64, sin contenedor Alpine y sin docker**.

Lleva un wrapper de Gradle commiteado (la CI no tiene gradle instalado), instala
el **NDK r28** —que alinea a 16 KB por defecto— y cuelga el paquete de la
etiqueta como hace `mini.yml`.

Dos cosas deliberadas:

- **Comprueba lo que hay dentro del APK**, no sólo que compiló: que
  `libtunante_decoder.so` es un ELF aarch64 de verdad y que las tres bibliotecas
  están. Todo el diseño se apoya en que ese binario llegue entero y ejecutable;
  si el empaquetado deja de extraerlo, tiene que saltar ahí y no en un teléfono.
- **No corre la prueba de formatos en Android**, y el fichero explica por qué:
  no hay un Android arm64 donde correrla, y el emulador x86_64 ejecutaría C
  distinto. Un verde ahí no diría nada del móvil.

**Un agujero conocido, escrito donde toca:** el APK va firmado con una clave de
depuración nueva en cada ejecución, así que Android se niega a instalarlo
*encima* de uno anterior. Es el mismo problema que el `abuild-keygen` por
compilación del paquete de Alpine y quiere la misma respuesta: una clave estable
en un secreto. El flujo lo avisa con un `::warning::` en vez de dejar que se
descubra al segundo intento.

### Fase 6 — lo que decía el plan

- **Gradle + `cargo-ndk`**, que es lo que recomienda el README de
  `android-activity`. **No `cargo-apk`** (última publicación 0.10.0, de
  noviembre de 2023, sin soporte de la plataforma 36) y **no `xbuild`** (el
  repositorio se describe a sí mismo como *unmaintained*).
- NDK **r28c o r29**: desde r28 se alinea a 16 KB por defecto, que es la puerta
  de publicación de febrero de 2027.
- La CI puede correr en **x86_64**, cruzando. Al contrario que el trabajo de
  postmarketOS, no hace falta un runner aarch64 nativo ni el rodeo de Docker con
  Alpine, porque aquí se cruza de verdad en vez de compilar nativo. Debería
  salir un flujo más simple que `mini.yml`.
- El `.apk` se cuelga de la etiqueta igual que hoy. **Con nombre distinto del de
  Alpine** — ver arriba.

---

## Lo que este plan no hace

- **No toca `tunante-core`.** Está limpio: sin FFI, sin GUI, sin llamadas de
  plataforma. Sólo `rusqlite` empaquetado, que cruza a Android sin drama.
- **No mete el decodificador en el proceso.** Ver decisión 4.
- **No migra a SAF.** Ver «Lo que casi nos come».
- **No persigue Google Play.** Es la misma decisión que la anterior.
- **No comparte código de interfaz con `tunante-mini`.** Es el precio de la
  interfaz nativa, y se paga a sabiendas: dos interfaces que se parecen, con la
  lógica compartida por debajo. Cuando cambie una pantalla habrá que cambiarla
  dos veces.

## Riesgos abiertos, por orden de lo que dolería

Nótese lo corta que es esta lista comparada con la del plan anterior. Los dos
riesgos que la encabezaban —Skia sin alternativa en el Adreno 618, y la madurez
del IME de Slint en Android— **los ha borrado la decisión de interfaz nativa**.

1. ~~**El coste de escanear.**~~ **Medido y descartado** — 4 ms por fichero,
   igual que un `exec` vacío. Ver Fase 2.
2. **La duplicidad de cpal**, 0.16 y 0.17 en el mismo binario, con
   `ndk_context` inicializándose posiblemente dos veces. No ha dado guerra en
   las fases 0 y 1; sigue sin verificar que sea inocua.
3. **`boost.rs`.** `sched_setattr` con uclamp existe en los núcleos de Android
   —lo usa el propio sistema para sus hilos de interfaz— pero puede rechazarlo
   SELinux bajo el filtro seccomp de la app. Ya falla con elegancia, así que no
   rompe nada; y con la interfaz en Compose, el hilo que había que acelerar ya
   no es nuestro.
4. ~~**El mapa de MIME.**~~ **Medido y confirmado**: los formatos de consola no
   se indexan como audio. Sólo importa si algún día se replantea Play, y lo que
   dice es que entonces habría que reescribir la capa de biblioteca sobre SAF.

---

## Resumen para quien no lea lo de arriba

| Parte | Coste |
|---|---|
| Cores de C emulados | ✅ hecho — 3 líneas de enlazado, 1 tamaño de pila, 2 ficheros de toolchain |
| `tunante-core` | nada |
| Decodificador fuera de proceso | ✅ hecho — 1 función, más `LD_LIBRARY_PATH` |
| Salida de audio | ✅ hecho — inicializar `ndk_context` a mano |
| Biblioteca y rutas | ✅ hecho — mover la ruta de la base de datos |
| Puente JNI | ✅ hecho — superficie JSON |
| Interfaz | 🟡 primera versión en pie; falta relleno |
| Servicio, MediaSession, foco de audio | ✅ hecho — framework, sin AndroidX |
| Empaquetado y CI | ✅ hecho — x86_64, sin runner ARM |

**Un mes de trabajo, sin ningún callejón sin salida conocido.** Lo raro de este
port es que la parte que asusta —1,7 GB de C de emuladores de consola— es la
barata, y la parte aburrida —el ciclo de vida de una app de Android— es la cara.

---

## La clave de firma

Generada el 2026-08-27. **Guárdala**: si se pierde, ninguna versión futura de la
app se puede instalar encima de una anterior nunca más. Android identifica una
app por su certificado, no por su nombre de paquete, y el único camino sería
desinstalar — perdiendo la biblioteca escaneada. No hay recuperación ni forma de
rotarla fuera de Google Play.

```
~/.android/tunante-release.jks        PKCS12, RSA 4096, 30 años, alias "tunante"
~/.android/tunante-release.pass       la contraseña, en texto plano, modo 600
SHA-256  e9:30:2e:1f:23:f3:68:b4:6b:9e:cd:b7:5a:c0:e7:bd:
         29:77:79:49:67:d6:d4:59:fb:87:ed:98:e8:7b:f4:54
```

**Fuera del repositorio a propósito**, y ahí se queda: una clave privada
commiteada es una clave pública.

La CI la recibe en dos secretos, `ANDROID_KEYSTORE_BASE64` y
`ANDROID_KEYSTORE_PASSWORD`. Sin ellos sigue compilando, pero en `debug`, que se
instala y no se puede actualizar por encima — un release **sin firmar** no se
instalaría en absoluto, que es peor.

`app/build.gradle.kts` sólo enchufa la configuración de firma si el fichero
existe de verdad, así que un clon nuevo sin la clave compila igual.
