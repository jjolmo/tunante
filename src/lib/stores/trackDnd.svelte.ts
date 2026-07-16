// Estado compartido para arrastrar pistas de la biblioteca a destinos de la
// barra lateral (playlists / crear playlist).
//
// Por qué existe: en Linux/WebKitGTK, Tauri con `dragDropEnabled: true` (que
// necesitamos para importar carpetas soltándolas desde el explorador) DESACTIVA
// el evento `drop` del DOM. Los eventos del lado ORIGEN (`dragstart`, `drag`,
// `dragend`) sí disparan, así que resolvemos el destino desde el origen usando
// `document.elementFromPoint` en vez de depender del `drop`. Así conviven el
// arrastre interno y el file-drop nativo. Ver TrackList (origen) y Sidebar
// (destinos).
class TrackDnd {
	ids = $state<string[]>([]);
	active = $state(false);
	hoverPlaylistId = $state<string | null>(null);
	hoverCreate = $state(false);
	// Lo pone a true un `drop` real del DOM (macOS/Windows) para que `dragend`
	// no vuelva a añadir las pistas (evita el doble commit).
	committed = false;
	// Canal: TrackList pide a Sidebar que abra el input de "crear playlist".
	createRequest = $state<string[] | null>(null);
}

export const trackDnd = new TrackDnd();
